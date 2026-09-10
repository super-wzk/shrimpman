use crate::config::SignEncoding;
mod model;
mod view;

use crate::model::{PasswordCredentials, SignInSuccess};
use crate::{credentials::CredentialStore, sign};
use model::{Effect, Message, Model};
use shrimpman_domain::character::CharacterId;
use std::sync::mpsc::{self, Receiver, Sender};

pub(crate) struct LaunchRequest {
    pub(crate) credentials: PasswordCredentials,
    pub(crate) sign_in: SignInSuccess,
    pub(crate) selected_character_id: CharacterId,
}

pub(crate) fn run(
    client: sign::Client,
    credential_store: CredentialStore,
    encoding: SignEncoding,
) -> Result<Option<LaunchRequest>, String> {
    let mut launch_request = None;
    let output = &mut launch_request;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([620.0, 440.0])
            .with_min_inner_size([440.0, 360.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Shrimpman MHF Launcher",
        options,
        Box::new(move |creation_context| {
            mhf_font::install(&creation_context.egui_ctx);
            egui_hunter::Theme::default().apply(&creation_context.egui_ctx);
            Ok(Box::new(EframeApp::new(
                client,
                credential_store,
                encoding,
                output,
                &creation_context.egui_ctx,
            )))
        }),
    )
    .map_err(|error| format!("failed to run launcher UI: {error}"))?;
    Ok(launch_request)
}

struct EframeApp<'a> {
    model: Model,
    view: view::View,
    client: sign::Client,
    credential_store: CredentialStore,
    messages: Receiver<Message>,
    message_sender: Sender<Message>,
    launch_request: &'a mut Option<LaunchRequest>,
}

impl<'a> EframeApp<'a> {
    fn new(
        client: sign::Client,
        credential_store: CredentialStore,
        encoding: SignEncoding,
        launch_request: &'a mut Option<LaunchRequest>,
        context: &egui::Context,
    ) -> Self {
        let (message_sender, messages) = mpsc::channel();
        let mut view = view::View::new(encoding);
        let model = match credential_store.read() {
            Ok(credentials) => Model::sign_in(credentials),
            Err(error) => {
                eprintln!("{error}");
                view.notify_error(context, "无法读取已保存的密码，请输入账号和密码继续。");
                Model::default()
            }
        };
        Self {
            model,
            view,
            client,
            credential_store,
            messages,
            message_sender,
            launch_request,
        }
    }

    fn dispatch(&mut self, message: Message, context: &egui::Context) {
        let (model, effect) = std::mem::take(&mut self.model).update(message);
        self.model = model;
        if let Some(effect) = effect {
            self.execute(effect, context);
        }
    }

    fn execute(&mut self, effect: Effect, context: &egui::Context) {
        match effect {
            Effect::NotifyError(message) => self.view.notify_error(context, &message),
            Effect::SignIn {
                credentials,
                remember_password,
            } => {
                let sender = self.message_sender.clone();
                let repaint_context = context.clone();
                let credential_store = self.credential_store.clone();
                let credentials_to_store = PasswordCredentials {
                    username: credentials.username.clone(),
                    password: credentials.password.clone(),
                };
                let result = self.client.sign_in(&credentials, move |result| {
                    let credential_error = if result.is_ok() {
                        let credential_result = if remember_password {
                            credential_store.write(&credentials_to_store)
                        } else {
                            credential_store.delete()
                        };
                        credential_result.err()
                    } else {
                        None
                    };
                    let _ = sender.send(Message::SignedIn {
                        result,
                        credential_error,
                    });
                    repaint_context.request_repaint();
                });
                if let Err(error) = result {
                    self.dispatch(
                        Message::SignedIn {
                            result: Err(error),
                            credential_error: None,
                        },
                        context,
                    );
                }
            }
            Effect::CreateCharacter {
                credentials,
                session_id,
                session_token,
            } => {
                let sender = self.message_sender.clone();
                let repaint_context = context.clone();
                let result = self.client.create_character(
                    &credentials,
                    session_id,
                    session_token,
                    move |result| {
                        let _ = sender.send(Message::CharacterCreated(result));
                        repaint_context.request_repaint();
                    },
                );
                if let Err(error) = result {
                    self.dispatch(Message::CharacterCreated(Err(error)), context);
                }
            }
            Effect::DeleteCharacter {
                session_id,
                session_token,
                character_id,
            } => {
                let sender = self.message_sender.clone();
                let repaint_context = context.clone();
                let result = self.client.delete_character(
                    session_id,
                    session_token,
                    character_id,
                    move |result| {
                        let _ = sender.send(Message::CharacterDeleted(result));
                        repaint_context.request_repaint();
                    },
                );
                if let Err(error) = result {
                    self.dispatch(Message::CharacterDeleted(Err(error)), context);
                }
            }
            Effect::Launch(request) => {
                *self.launch_request = Some(request);
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn receive_messages(&mut self, context: &egui::Context) {
        while let Ok(message) = self.messages.try_recv() {
            self.dispatch(message, context);
        }
    }
}

impl eframe::App for EframeApp<'_> {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_messages(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(message) = self.view.show(&mut self.model, ui) {
            self.dispatch(message, ui.ctx());
        }
    }
}

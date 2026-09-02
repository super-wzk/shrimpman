mod model;
mod theme;
mod view;

use crate::{credentials::CredentialStore, http};
use eframe::egui;
use model::{Effect, Message, Model};
use shrimpman_domain::character::CharacterId;
use shrimpman_mhf_launcher::{PasswordCredentials, SignInSuccess};
use std::sync::mpsc::{self, Receiver, Sender};

pub(crate) struct LaunchRequest {
    pub(crate) credentials: PasswordCredentials,
    pub(crate) sign_in: SignInSuccess,
    pub(crate) selected_character_id: CharacterId,
}

pub(crate) fn run(
    client: http::Client,
    credential_store: CredentialStore,
) -> Result<Option<LaunchRequest>, String> {
    let mut launch_request = None;
    let app = EframeApp::new(client, credential_store, &mut launch_request);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 520.0])
            .with_min_inner_size([440.0, 430.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Shrimpman MHF Launcher",
        options,
        Box::new(move |creation_context| {
            theme::install(&creation_context.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| format!("failed to run launcher UI: {error}"))?;
    Ok(launch_request)
}

struct EframeApp<'a> {
    model: Model,
    client: http::Client,
    credential_store: CredentialStore,
    messages: Receiver<Message>,
    message_sender: Sender<Message>,
    launch_request: &'a mut Option<LaunchRequest>,
}

impl<'a> EframeApp<'a> {
    fn new(
        client: http::Client,
        credential_store: CredentialStore,
        launch_request: &'a mut Option<LaunchRequest>,
    ) -> Self {
        let (message_sender, messages) = mpsc::channel();
        let model = match credential_store.read() {
            Ok(credentials) => Model::sign_in(credentials, None),
            Err(error) => Model::sign_in(None, Some(error)),
        };
        Self {
            model,
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
                session_id,
                session_token,
            } => {
                let sender = self.message_sender.clone();
                let repaint_context = context.clone();
                let result =
                    self.client
                        .create_character(session_id, session_token, move |result| {
                            let _ = sender.send(Message::CharacterCreated(result));
                            repaint_context.request_repaint();
                        });
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
        if let Some(message) = view::show(&mut self.model, ui) {
            self.dispatch(message, ui.ctx());
        }
    }
}

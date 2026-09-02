mod model;
mod theme;
mod view;

use crate::http;
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

pub(crate) fn run(client: http::Client) -> Result<Option<LaunchRequest>, String> {
    let mut launch_request = None;
    let app = EframeApp::new(client, &mut launch_request);
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
    messages: Receiver<Message>,
    message_sender: Sender<Message>,
    launch_request: &'a mut Option<LaunchRequest>,
}

impl<'a> EframeApp<'a> {
    fn new(client: http::Client, launch_request: &'a mut Option<LaunchRequest>) -> Self {
        let (message_sender, messages) = mpsc::channel();
        Self {
            model: Model::default(),
            client,
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
            Effect::SignIn(credentials) => {
                let sender = self.message_sender.clone();
                let repaint_context = context.clone();
                let result = self.client.sign_in(&credentials, move |result| {
                    let _ = sender.send(Message::SignedIn(result));
                    repaint_context.request_repaint();
                });
                if let Err(error) = result {
                    self.dispatch(Message::SignedIn(Err(error)), context);
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

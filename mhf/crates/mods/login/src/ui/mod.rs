use crate::config::SignEncoding;
mod model;
mod view;

use crate::model::{PasswordCredentials, SignInSuccess};
use crate::settings::{Settings, SettingsCategory};
use crate::{config, credentials::CredentialStore, sign};
use mhf_config::Config;
use model::{Effect, Message, Model};
use shrimpman_domain::character::CharacterId;
use std::sync::mpsc::{self, Receiver, Sender};

pub(crate) struct LaunchRequest {
    pub(crate) credentials: PasswordCredentials,
    pub(crate) sign_in: SignInSuccess,
    pub(crate) selected_character_id: CharacterId,
}

pub(crate) fn run(
    configuration: Config<'_>,
) -> Result<Option<(LaunchRequest, SignEncoding)>, String> {
    let mut launch_request = None;
    let output = &mut launch_request;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../../../apps/launcher/assets/icon.png"
                ))
                .expect("valid embedded launcher icon"),
            )
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
                configuration,
                output,
                &creation_context.egui_ctx,
            )?))
        }),
    )
    .map_err(|error| format!("failed to run launcher UI: {error}"))?;
    Ok(launch_request)
}

struct EframeApp<'a> {
    model: Model,
    view: view::View,
    configuration: Config<'a>,
    connection: Option<Connection>,
    settings: Option<Settings>,
    messages: Receiver<Message>,
    message_sender: Sender<Message>,
    launch_request: &'a mut Option<(LaunchRequest, SignEncoding)>,
}

struct Connection {
    client: sign::Client,
    credential_store: CredentialStore,
    encoding: SignEncoding,
}

impl Connection {
    fn load(configuration: Config<'_>) -> Result<Self, String> {
        let source = configuration
            .read("sign")
            .map_err(|error| error.to_string())?;
        let settings = config::load(&source)?;
        let client = sign::Client::new(&settings.endpoint, settings.encoding)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            credential_store: CredentialStore::new(&client.credential_target()),
            client,
            encoding: settings.encoding,
        })
    }
}

impl<'a> EframeApp<'a> {
    fn new(
        configuration: Config<'a>,
        launch_request: &'a mut Option<(LaunchRequest, SignEncoding)>,
        context: &egui::Context,
    ) -> Result<Self, String> {
        let (message_sender, messages) = mpsc::channel();
        let mut app = Self {
            model: Model::default(),
            view: view::View::default(),
            configuration,
            connection: None,
            settings: None,
            messages,
            message_sender,
            launch_request,
        };
        match Connection::load(configuration) {
            Ok(connection) => {
                if let Err(error) = app.use_connection(connection) {
                    app.notify_error(context, &error);
                }
            }
            Err(error) => {
                app.settings = Some(Settings::load(configuration, SettingsCategory::Server)?);
                app.notify_error(context, &error);
            }
        }
        Ok(app)
    }

    fn dispatch(&mut self, message: Message, context: &egui::Context) {
        match message {
            Message::OpenSettings(category) => {
                if self.model.can_configure() {
                    match Settings::load(self.configuration, category) {
                        Ok(settings) => self.settings = Some(settings),
                        Err(error) => self.notify_error(context, &error),
                    }
                }
                return;
            }
            Message::SaveSettings => {
                match self.save_settings() {
                    Ok(()) => self
                        .view
                        .notify_success(context, "设置已保存，本次启动生效。"),
                    Err(error) => self.notify_error(context, &error),
                }
                return;
            }
            Message::CancelSettings => {
                self.settings = None;
                return;
            }
            _ => {}
        }
        let (model, effect) = std::mem::take(&mut self.model).update(message);
        self.model = model;
        if let Some(effect) = effect {
            self.execute(effect, context);
        }
    }

    fn notify_error(&mut self, context: &egui::Context, error: &str) {
        eprintln!("{error}");
        self.view.notify_error(context, error);
    }

    fn use_connection(&mut self, connection: Connection) -> Result<(), String> {
        let credentials = connection.credential_store.read();
        // A new server must never inherit the previous session or credentials.
        self.view = view::View::new(connection.encoding);
        self.model = Model::default();
        self.connection = Some(connection);
        self.model = Model::sign_in(credentials.map_err(|error| {
            eprintln!("{error}");
            "无法读取已保存的密码，请输入账号和密码继续。".to_owned()
        })?);
        Ok(())
    }

    fn save_settings(&mut self) -> Result<(), String> {
        let Some(settings) = &self.settings else {
            return Ok(());
        };
        settings.save(self.configuration)?;
        let connection = if matches!(settings, Settings::Server { .. }) {
            let connection = Connection::load(self.configuration)?;
            let changed = self.connection.as_ref().is_none_or(|previous| {
                previous.client.credential_target() != connection.client.credential_target()
                    || previous.encoding != connection.encoding
            });
            changed.then_some(connection)
        } else {
            None
        };
        self.settings = None;
        if let Some(connection) = connection {
            self.use_connection(connection)
                .map_err(|error| format!("服务器地址已保存。{error}"))?;
        }
        Ok(())
    }

    fn execute(&mut self, effect: Effect, context: &egui::Context) {
        match effect {
            Effect::NotifyError(message) => self.view.notify_error(context, &message),
            Effect::SignIn {
                credentials,
                remember_password,
            } => {
                let Some(connection) = &self.connection else {
                    self.dispatch(
                        Message::SignedIn {
                            result: Err(sign::Error::InvalidRequest(
                                "请先在设置中填写有效的登录服务器地址。".into(),
                            )),
                            credential_error: None,
                        },
                        context,
                    );
                    return;
                };
                let sender = self.message_sender.clone();
                let repaint_context = context.clone();
                let credential_store = connection.credential_store.clone();
                let credentials_to_store = PasswordCredentials {
                    username: credentials.username.clone(),
                    password: credentials.password.clone(),
                };
                let result = connection.client.sign_in(&credentials, move |result| {
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
                let connection = self
                    .connection
                    .as_ref()
                    .expect("a character session has a Sign connection");
                let sender = self.message_sender.clone();
                let repaint_context = context.clone();
                let result = connection.client.create_character(
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
                let connection = self
                    .connection
                    .as_ref()
                    .expect("a character session has a Sign connection");
                let sender = self.message_sender.clone();
                let repaint_context = context.clone();
                let result = connection.client.delete_character(
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
                let connection = self
                    .connection
                    .as_ref()
                    .expect("a character session has a Sign connection");
                *self.launch_request = Some((request, connection.encoding));
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
        let message = match &mut self.settings {
            Some(settings) => self.view.show_settings(settings, ui),
            None => self.view.show(&mut self.model, ui),
        };
        if let Some(message) = message {
            self.dispatch(message, ui.ctx());
        }
    }
}

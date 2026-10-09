//! Native debug window. The UI owns drafts; the game owns live state and commands.

use super::{DebugControl, UiSettings, input, ui::DebugPanel};
use egui::ViewportCommand;
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows::Win32::{
    Foundation::HWND,
    UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

mod dialogs;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct WindowState {
    context: OnceLock<egui::Context>,
    visible: AtomicBool,
    stopping: AtomicBool,
    #[cfg(test)]
    native_window: std::sync::atomic::AtomicIsize,
}

impl WindowState {
    pub(super) fn toggle(&self) {
        let visible = !self.visible.fetch_xor(true, Ordering::AcqRel);
        let context = self.context.get().unwrap();
        context.send_viewport_cmd(ViewportCommand::Visible(visible));
        if visible {
            context.send_viewport_cmd(ViewportCommand::Focus);
        }
        context.request_repaint();
    }

    fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        let context = self.context.get().unwrap();
        context.send_viewport_cmd(ViewportCommand::Close);
        context.request_repaint();
    }
}

pub(super) struct DebugDesktop {
    state: Arc<WindowState>,
    thread: Option<JoinHandle<Result<(), String>>>,
}

impl DebugDesktop {
    pub(super) fn start(control: Arc<DebugControl>) -> Result<Self, String> {
        let state = Arc::new(WindowState::default());
        state.visible.store(true, Ordering::Release);
        let window = state.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("mhf-debug-ui".into())
            .spawn(move || {
                // Scale only this desktop UI thread; preserve the legacy game
                // thread's DPI virtualization and coordinate space.
                unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
                let ready = ready_tx.clone();
                let result = eframe::run_native(
                    "MHF 调试工具",
                    eframe::NativeOptions {
                        viewport: egui::ViewportBuilder::default()
                            .with_inner_size([1000.0, 760.0])
                            .with_min_inner_size([440.0, 360.0]),
                        centered: true,
                        event_loop_builder: Some(Box::new(|builder| {
                            // The native game keeps its original DPI behavior.
                            builder.with_any_thread(true).with_dpi_aware(false);
                        })),
                        ..Default::default()
                    },
                    Box::new(move |creation| {
                        let context = creation.egui_ctx.clone();
                        mhf_font::install(&context);
                        egui_hunter::Theme::default()
                            .density(egui_hunter::Density::Compact)
                            .apply(&context);
                        window.context.set(context).unwrap();
                        let app = DebugApp {
                            settings: control.ui_settings(),
                            panel: DebugPanel::new(control.clone()),
                            control,
                            window,
                        };
                        let _ = ready.send(Ok(()));
                        Ok(Box::new(app))
                    }),
                )
                .map_err(|error| format!("无法打开调试窗口：{error}"));
                if let Err(error) = &result {
                    let _ = ready_tx.send(Err(error.clone()));
                }
                result
            })
            .map_err(|error| format!("无法启动调试窗口线程：{error}"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                state,
                thread: Some(thread),
            }),
            ready => {
                let result = thread
                    .join()
                    .map_err(|_| "调试窗口线程意外终止".to_owned())?;
                Err(match ready {
                    Ok(Err(error)) => error,
                    _ => result
                        .err()
                        .unwrap_or_else(|| "调试窗口未完成初始化".into()),
                })
            }
        }
    }

    pub(super) fn state(&self) -> Arc<WindowState> {
        self.state.clone()
    }

    pub(super) fn stop(&mut self) -> Result<(), String> {
        if let Some(thread) = self.thread.take() {
            self.state.stop();
            thread
                .join()
                .map_err(|_| "调试窗口线程意外终止".to_owned())??;
        }
        Ok(())
    }
}

impl Drop for DebugDesktop {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("debug window cleanup failed: {error}");
        }
    }
}

struct DebugApp {
    control: Arc<DebugControl>,
    window: Arc<WindowState>,
    settings: UiSettings,
    panel: DebugPanel,
}

impl eframe::App for DebugApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        if self.window.stopping.load(Ordering::Acquire) {
            context.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        if context.input(|input| input.viewport().close_requested()) {
            context.send_viewport_cmd(ViewportCommand::CancelClose);
            self.window.visible.store(false, Ordering::Release);
            context.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        if input::toggle_panel(context) {
            self.window.toggle();
        }
        // Snapshot refresh is independent of game Present, including AI pauses.
        context.request_repaint_after(Duration::from_millis(33));
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let owner = frame
            .window_handle()
            .ok()
            .and_then(|handle| match handle.as_raw() {
                RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
                _ => None,
            });
        #[cfg(test)]
        if let Some(owner) = owner {
            self.window
                .native_window
                .store(owner.0 as isize, Ordering::Release);
        }
        if self.window.stopping.load(Ordering::Acquire)
            || !self.window.visible.load(Ordering::Acquire)
        {
            return;
        }
        let snapshot = self.control.snapshot();
        self.panel.show(ui, &snapshot, &mut self.settings.input);
        self.settings.hud_target = self.panel.hud_target();
        self.control.set_ui_settings(self.settings);
        if !self.window.stopping.load(Ordering::Acquire)
            && self.window.visible.load(Ordering::Acquire)
            && let Some(owner) = owner
            && let Some(json) = self.panel.take_recording_save()
        {
            let result = dialogs::save_recording(frame, owner, &json, &self.window);
            self.panel.recording_save_finished(result);
        }
    }
}

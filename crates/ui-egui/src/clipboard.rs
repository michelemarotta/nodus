//! The system clipboard: Cut/Copy/Paste key presses, and the drawing clipboard mirrored as DXF
//! text so it can be pasted into another CADCraft window.
//!
//! egui turns Cmd/Ctrl+C, X and V into `Copy`, `Cut` and `Paste(text)` events instead of key
//! presses, so the clipboard shortcuts are read from those. A `Paste` arrives only when the system
//! clipboard holds text; every copy therefore puts its DXF text there.

use std::hash::{DefaultHasher, Hash, Hasher};

use cadcraft_engine::cmd::clipboard;

use crate::CadApp;

/// egui memory: the clipboard copy last mirrored, and the hash of the text it became.
fn serial_id() -> egui::Id {
    egui::Id::unique("cc_clipboard_serial")
}
fn text_id() -> egui::Id {
    egui::Id::unique("cc_clipboard_text")
}

/// Line endings normalised (pasted text arrives with `\n`), so our own text is recognised.
fn hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.replace("\r\n", "\n").hash(&mut h);
    h.finish()
}

/// Put a new copy (COPYCLIP, CUTCLIP, COPYBASE from anywhere) on the system clipboard, once.
pub fn mirror(app: &CadApp, ctx: &egui::Context) {
    let Some(src) = &app.session.clipboard_source else { return };
    if src.uid.is_none() || ctx.data(|d| d.get_temp::<u64>(serial_id())) == Some(src.serial) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(serial_id(), src.serial));
    if let Some(text) = clipboard::system_text(&app.session) {
        ctx.data_mut(|d| d.insert_temp(text_id(), hash(&text)));
        ctx.copy_text(text);
    }
}

/// Route clipboard events: CADCraft DXF objects use drawing paste commands; ordinary text
/// goes into the command-line buffer, including AutoLISP expressions. Focused text widgets
/// handle their own paste because menus::shortcuts returns before calling this function.
pub fn shortcut_commands(app: &mut CadApp, ctx: &egui::Context) -> Vec<&'static str> {
    let (events, shift) = ctx.input(|i| {
        let ev: Vec<egui::Event> =
            i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))).cloned().collect();
        (ev, i.modifiers.shift)
    });
    let mut out = Vec::new();
    for e in events {
        match e {
            egui::Event::Copy => out.push(if shift { "copybase" } else { "copyclip" }),
            egui::Event::Cut => out.push("cutclip"),
            egui::Event::Paste(text) => {
                let h = hash(&text);
                // A copy mirrored from this CAD window is already in the drawing clipboard.
                // DXF copied from another CAD window must first be decoded.
                let cached_cad_copy = ctx.data(|d| d.get_temp::<u64>(text_id())) == Some(h) && !app.session.clipboard.is_empty();
                let is_cad_copy = cached_cad_copy || clipboard::load_system_text(&mut app.session, &text);
                if is_cad_copy {
                    if !cached_cad_copy {
                        ctx.data_mut(|d| d.insert_temp(text_id(), h));
                    }
                    out.push(if shift { "pasteblock" } else { "pasteclip" });
                } else {
                    // The command line is painted rather than an egui text field: it has no
                    // keyboard focus of its own. Paste ordinary text there, not as CAD objects.
                    app.cmd.preedit.clear();
                    app.cmd.buffer.push_str(&text);
                    app.cmd.note_typing();
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use cadcraft_engine::Session;

    /// Ctrl+V with an AutoLISP expression must not invoke PASTECLIP on an empty CAD clipboard.
    #[test]
    fn plain_text_paste_goes_to_command_line() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.cmd.buffer = "(setq ".to_string();
        let ctx = egui::Context::default();
        let mut commands = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput { events: vec![egui::Event::Paste("e (entlast))".into())], ..Default::default() },
            |ui| {
                commands = shortcut_commands(&mut app, ui.ctx());
            },
        );
        output.textures_delta.clear();
        assert!(commands.is_empty(), "plain text must not invoke CAD paste");
        assert_eq!(app.cmd.buffer, "(setq e (entlast))");
        assert!(app.session.clipboard.is_empty(), "text paste must not alter the drawing clipboard");
    }
}

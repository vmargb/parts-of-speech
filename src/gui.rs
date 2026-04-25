use eframe::egui::{
    self, Color32, FontId, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2,
};
use crate::state::{AppState, PlaybackState, Command};
use crate::RecorderApp;
use crate::themes::{ThemeKind, palette_for};


// -- eframe::App ---------------------------------------------------------------
impl eframe::App for RecorderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Capture wall time once per frame so sub-widgets can read it without
        // calling ctx.input() while holding any locks (deadlock prevention).
        // This must happen before any lock is taken this frame.
        self.frame_time = ctx.input(|i| i.time);

        {
            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
            let is_playing_now = rec.playback_state == PlaybackState::Playing;
            if matches!(rec.state, AppState::Recording) || is_playing_now {
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            }
            // Detect the Idle→Playing edge and stamp the wall-clock start time.
            // Lock is dropped before any further ctx calls to avoid deadlocks.
            if is_playing_now && !self.prev_is_playing {
                self.playback_wall_start = self.frame_time;
            }
            self.prev_is_playing = is_playing_now;
        }

        // Drain a pending seek now that the audio thread has gone Idle.
        // We must not hold the recorder lock while calling play_segment_from.
        if let Some((seg_idx, offset)) = self.pending_seek.take() {
            let is_idle_now = {
                let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
                rec.playback_state == PlaybackState::Idle
            };
            if is_idle_now {
                self.playback_display_offset = offset;
                self.play_segment_from(seg_idx, offset, None);
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            } else {
                // Still stopping put it back and wait one more frame
                self.pending_seek = Some((seg_idx, offset));
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        // keep palette in sync with current theme (struct copy each frame)
        self.palette = palette_for(&self.theme);
        self.apply_theme(ctx);
        // Refresh waveform peak caches before any drawing.
        // Uses try_lock internally safe to call here with no locks held.
        self.update_waveform_caches();
        self.handle_keyboard(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.palette.bg))
            .show(ctx, |ui| {
                let avail = ui.available_width();
                
                // safely max out at 960px on wide screens.
                let max_width = 960.0;
                let side_pad = ((avail - max_width) / 2.0).max(16.0);
                
                egui::Frame::none()
                    .inner_margin(egui::Margin { left: side_pad, right: side_pad + 55.0, top: 24.0, bottom: 20.0 })
                    .show(ui, |ui| {
                        // all elements share the exact same width bounds intrinsically.
                        self.draw_header(ui, ctx);
                        ui.add_space(18.0);
                        self.draw_transport_card(ui, ctx);
                        
                        ui.add_space(16.0);
                        self.draw_segment_list(ui, ctx);
                        ui.add_space(14.0);
                        
                        self.draw_footer(ui, ctx);
                    });
                if self.show_keybindings {
                    self.draw_keybindings_overlay(ctx);
                }
                if self.show_settings {
                    self.draw_settings_overlay(ctx);
                }
            });
    }
}

// SeekKind is returned by draw_segment_row to signal what playback to trigger
// Kept in gui.rs because it is purely a GUI-internal signalling type
// draw_segment_row passes one of these back to draw_segment_list (which owns
// the mutable borrow of self) so that playback is started *after* the
// ScrollArea has released its borrow.
//
//  EdgePreview(bool)  play trim_preview_secs from the start (true) or
//                      end (false) of the segment, fired automatically
//                      right after a trim so the user can hear the edit.
//  SeekTo(f32)        play from `offset_secs` to the end of the segment,
//                      fired when the user clicks or releases a drag on
//                      the seek bar.
#[derive(Clone, Copy)]
pub enum SeekKind {
    EdgePreview(bool), // true = from start, false = from end
    SeekTo(f32),       // offset in seconds from the start of the segment
}

// -- Helpers -------------------------------------------------------------------
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        255,
    )
}

// Slightly tint a color toward white for hover states
fn lighten(c: Color32, amt: u8) -> Color32 {
    Color32::from_rgb(
        c.r().saturating_add(amt),
        c.g().saturating_add(amt),
        c.b().saturating_add(amt),
    )
}

// Slightly tint a color toward black
fn darken(c: Color32, amt: u8) -> Color32 {
    Color32::from_rgb(
        c.r().saturating_sub(amt),
        c.g().saturating_sub(amt),
        c.b().saturating_sub(amt),
    )
}

// Produce a ghosted/inactive color that works on both dark and light backgrounds.
// On dark backgrounds (sum < 384) we lighten; on light backgrounds we darken.
// This prevents inactive header buttons from becoming invisible white-on-white.
fn ghost_inactive(bg: Color32, amt: u8) -> Color32 {
    let lum = bg.r() as u16 + bg.g() as u16 + bg.b() as u16;
    if lum > 384 { darken(bg, amt) } else { lighten(bg, amt) }
}


// -- RecorderApp impl ----------------------------------------------------------
impl RecorderApp {
    fn apply_theme(&self, ctx: &egui::Context) {
        let p = &self.palette;

        // detect if in a light theme by checking background brightness
        // If R+G+B > 384 (approx midpoint of 0-765), treat it as light
        let is_light = (p.bg.r() as u16 + p.bg.g() as u16 + p.bg.b() as u16) > 384;
        // start with a fresh base visual state matching the themes brightness then adjsut
        let mut v = if is_light {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        // base assignments
        v.panel_fill = p.bg;
        v.window_fill = p.surf;
        v.extreme_bg_color = p.surf2;

        if is_light {
            v.widgets.noninteractive.bg_fill = p.surf3; 
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
        } else {
            v.widgets.noninteractive.bg_fill = p.surf2;
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.dim);
        }

        // consistent for both themes
        v.widgets.inactive.bg_fill = p.surf2;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);

        v.widgets.hovered.bg_fill = p.surf3;
        v.widgets.hovered.fg_stroke = Stroke::new(1.5, p.text);

        v.widgets.active.bg_fill = p.surf3;
        v.widgets.active.fg_stroke = Stroke::new(2.0, p.text);

        v.selection.bg_fill = Color32::from_rgba_unmultiplied(
            p.play.r(), p.play.g(), p.play.b(), 40);

        v.override_text_color = Some(p.text);

        ctx.set_visuals(v);
    }

    // -- Header ----------------------------------------------------------------
    fn draw_header(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let (text, dim, rec, amber, mono, border, _blue) = {
            let p = &self.palette;
            (p.text, p.dim, p.rec, p.amber, p.mono, p.border, p.blue)
        }; // borrow dropped here

        ui.horizontal(|ui| {
            ui.label(RichText::new("PARTS").font(FontId::monospace(14.0)).color(text).strong());
            ui.add_space(3.0);
            ui.label(RichText::new("OF").font(FontId::monospace(14.0)).color(dim));
            ui.add_space(3.0);
            ui.label(RichText::new("SPEECH").font(FontId::monospace(14.0)).color(rec).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (gear_r, gear_resp) = ui.allocate_exact_size(Vec2::new(52.0, 16.0), Sense::click());
                let gear_hov = gear_resp.hovered();
                let gear_col = if self.show_settings { amber }
                    else if gear_hov { dim } else { ghost_inactive(self.palette.bg, 38) };
                ui.painter().text(gear_r.center(), egui::Align2::CENTER_CENTER,
                    "⚙ settings", FontId::monospace(9.0), gear_col);
                if gear_resp.clicked() {
                    self.show_settings = !self.show_settings;
                    self.show_keybindings = false;
                }
                ui.add_space(10.0);
                let (kb_r, kb_resp) = ui.allocate_exact_size(Vec2::new(40.0, 16.0), Sense::click());
                let kb_col = if self.show_keybindings { mono }
                    else if kb_resp.hovered() { dim } else { ghost_inactive(self.palette.bg, 38) };
                ui.painter().text(kb_r.center(), egui::Align2::CENTER_CENTER,
                    "? help", FontId::monospace(9.0), kb_col);
                if kb_resp.clicked() {
                    self.show_keybindings = !self.show_keybindings;
                    self.show_settings = false;
                }
                ui.add_space(10.0);
                self.draw_status_badge(ui);
            });
        });
        ui.add_space(10.0);
        let r = ui.available_rect_before_wrap();
        ui.painter().line_segment([r.min, Pos2::new(r.max.x, r.min.y)],
            Stroke::new(1.0, border));
        ui.add_space(1.0);
    }

    fn draw_status_badge(&self, ui: &mut egui::Ui) {
        // read time BEFORE locking the recorder. ui.input() internally
        // acquires egui's context read-lock; holding our recorder mutex
        // at the same time creates a lock-ordering inversion with egui's
        // repaint machinery and causes intermittent deadlocks.
        let t = ui.input(|i| i.time) as f32;
        let p = &self.palette;
        let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
        let (text, col) = match (&rec.state, &rec.playback_state) {
            (AppState::Recording, _)    => ("REC",    p.rec),
            (_, PlaybackState::Playing) => ("PLAY",   p.play),
            (AppState::Reviewing, _)    => ("REVIEW", p.amber),
            _                           => ("IDLE",   p.muted),
        };
        let alpha = if matches!(rec.state, AppState::Recording) {
            ((t * 2.8).sin() * 0.42 + 0.58).clamp(0.0, 1.0)
        } else { 1.0 };
        let col = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), (alpha * 255.0) as u8);
        ui.label(RichText::new(text).font(FontId::monospace(11.0)).color(col).strong());
    }

    // -- Transport Card --------------------------------------------------------
    fn draw_transport_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (state_str, is_playing, seg_count, cur_samples, sample_rate, can_undo, can_redo) = {
            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
            let s = match &rec.state {
                AppState::Idle      => "idle",
                AppState::Recording => "recording",
                AppState::Reviewing => "reviewing",
            };
            (s,
             rec.playback_state == PlaybackState::Playing,
             rec.get_segment_count(),
             rec.current.as_ref().map(|s| s.samples.len()).unwrap_or(0),
             rec.project.sample_rate,
             !rec.history.is_empty() || rec.previous_current.is_some(),
             rec.history_index < rec.history.len().saturating_sub(1) || rec.next_current.is_some())
        };

        let p = &self.palette;

        egui::Frame {
            fill: p.surf, rounding: Rounding::same(10.0),
            stroke: Stroke::new(1.0, p.border),
            inner_margin: egui::Margin { left: 20.0, right: 20.0, top: 16.0, bottom: 16.0 },
            ..Default::default()
        }
        .show(ui, |ui| {
            // -- Timer ---------------------------------------------------------
            let secs = cur_samples as f32 / sample_rate.max(1) as f32;
            let timer_col = match state_str {
                "recording" => p.rec, "reviewing" => p.amber,
                _ => Color32::from_rgb(
                    (p.bg.r() as u16 + p.surf3.r() as u16 / 2) as u8,
                    (p.bg.g() as u16 + p.surf3.g() as u16 / 2) as u8,
                    (p.bg.b() as u16 + p.surf3.b() as u16 / 2) as u8,
                ),
            };
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(if state_str != "idle" {
                    format!("{:02}:{:02}.{:02}", (secs/60.0) as u32, (secs%60.0) as u32, ((secs%1.0)*100.0) as u32)
                } else { "00:00.00".into() })
                    .font(FontId::monospace(52.0)).color(timer_col).strong());
                ui.add_space(2.0);
                let sub = match state_str {
                    "recording" => format!("recording  --  {} samples captured", cur_samples),
                    "reviewing" => "listen -- confirm or reject -- try again".into(),
                    _ if seg_count > 0 =>
                        format!("{} segment{}  --  ready", seg_count, if seg_count == 1 { "" } else { "s" }),
                    _ => "press RECORD to begin".into(),
                };
                ui.label(RichText::new(sub).font(FontId::monospace(10.0)).color(p.dim));

                // -- Live waveform -------------------------------------------
                // Only rendered while Recording or Reviewing (has sample data)
                // Peaks are pre-computed in update_waveform_caches() so just
                // read self.live_peaks here (VecDeque<f32> owned by RecorderApp)
                // no shared state, so no locks or clones
                if !self.live_peaks.is_empty() && state_str != "idle" {
                    ui.add_space(6.0);
                    let wf_h = 38.0_f32;
                    let (wf_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), wf_h),
                        Sense::hover(),
                    );

                    // red for REC, amber for REVIEW
                    let wf_col = if state_str == "recording" { p.rec } else { p.amber };
                    let n = self.live_peaks.len();
                    let bar_w = wf_rect.width() / n as f32;
                    let cy = wf_rect.center().y;
                    let half_h = wf_h * 0.46;

                    for (i, &peak) in self.live_peaks.iter().enumerate() {
                        let x = wf_rect.min.x + i as f32 * bar_w;
                        // map linear amplitude to dB scale for display,
                        // similar to other how DAWs render waveforms
                        // -60 dB floor: 0.0,  0 dB (full scale): 1.0
                        // Quiet audio that sits around -30 dB
                        // maps to around 0.5, giving a clearly visible bar instead
                        let display = if peak < 1e-6 { 0.0_f32 } else {
                            ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
                        };
                        // minimum bar height of 1.5 px so silent buckets still
                        // leave a faint baseline
                        let bar_h = (display * half_h * 2.0).max(1.5);
                        let bar_rect = Rect::from_center_size(
                            Pos2::new(x + bar_w * 0.5, cy),
                            Vec2::new((bar_w - 0.8).max(0.5), bar_h),
                        );
                        // alpha also tracks the dB-scaled value so quiet passages
                        // are translucent and loud peaks are more opaque.
                        let alpha = ((display * 180.0) + 40.0).min(255.0) as u8;
                        ui.painter().rect_filled(
                            bar_rect,
                            Rounding::ZERO,
                            Color32::from_rgba_unmultiplied(
                                wf_col.r(), wf_col.g(), wf_col.b(), alpha),
                        );
                    }
                }
            });

            ui.add_space(16.0);

            // -- Primary row ---------------------------------------------------
            // during playback a STOP PLAYBACK button replaces the
            // greyed-out controls so the user always can stop playback
            // the buttons are still allocated (so layout doesn't jump)
            ui.horizontal(|ui| {
                let gap = 8.0_f32;
                let w   = ((ui.available_width() - gap * 4.0) / 5.0).max(1.0);
                let h   = 48.0_f32;

                if is_playing {
                    // left two slots: dimmed placeholders
                    self.transport_btn(ui, ctx, "RECORD",  w, h, false, p.rec,  || {});
                    ui.add_space(gap);
                    self.transport_btn(ui, ctx, "STOP",    w, h, false, p.muted, || {});
                    ui.add_space(gap);
                    // centre slot: the actual stop-playback button
                    self.transport_btn(ui, ctx, "■ STOP PLAYBACK", w, h, true, p.amber,
                        || self.handle_command(Command::StopPlayback));
                    ui.add_space(gap);
                    self.transport_btn(ui, ctx, "CONFIRM", w, h, false, p.play,  || {});
                    ui.add_space(gap);
                    self.transport_btn(ui, ctx, "REJECT",  w, h, false, p.rec,   || {});
                } else {
                    self.transport_btn(ui, ctx, "RECORD", w, h,
                        state_str == "idle", p.rec,
                        || self.handle_command(Command::StartRecording));
                    ui.add_space(gap);
                    self.transport_btn(ui, ctx, "STOP", w, h,
                        state_str == "recording", p.muted,
                        || self.handle_command(Command::StopRecording));
                    ui.add_space(gap);
                    let listen_lbl = if state_str == "reviewing" { "LISTEN" } else { "PLAY" };
                    self.transport_btn(ui, ctx, listen_lbl, w, h,
                        state_str == "reviewing" || seg_count > 0, p.play,
                        || {
                            if state_str == "reviewing" { self.play_current_segment(); }
                            else if seg_count > 0 { self.handle_command(Command::PlaySegment(seg_count - 1)); }
                        });
                    ui.add_space(gap);
                    self.transport_btn(ui, ctx, "CONFIRM", w, h,
                        state_str == "reviewing", p.play,
                        || self.handle_command(Command::Approve));
                    ui.add_space(gap);
                    self.transport_btn(ui, ctx, "REJECT", w, h,
                        state_str == "reviewing", p.rec,
                        || self.handle_command(Command::Reject));
                }
            });

            ui.add_space(8.0);

            // -- Secondary row: TRY AGAIN  PLAY ALL  UNDO  REDO ---------------
            ui.horizontal(|ui| {
                let gap = 8.0_f32;
                let w   = ((ui.available_width() - gap * 3.0) / 4.0).max(1.0);
                let h   = 34.0_f32;

                self.transport_btn(ui, ctx, "TRY AGAIN", w, h,
                    state_str == "reviewing" && !is_playing, p.muted,
                    || self.handle_command(Command::RetryCurrentTake));
                ui.add_space(gap);
                // PLAY ALL is disabled while already playing (can't stack playback)
                // but not blocked by any other state
                self.transport_btn(ui, ctx, "PLAY ALL", w, h,
                    seg_count > 0 && !is_playing && state_str == "idle", p.muted,
                    || self.handle_command(Command::PlayAll));
                ui.add_space(gap);
                // Undo/redo remain blocked during playback — they mutate project data
                self.transport_btn(ui, ctx, "<< UNDO", w, h,
                    can_undo && state_str == "idle" && !is_playing, p.muted,
                    || self.handle_command(Command::Undo));
                ui.add_space(gap);
                self.transport_btn(ui, ctx, "REDO >>", w, h,
                    can_redo && state_str == "idle" && !is_playing, p.muted,
                    || self.handle_command(Command::Redo));
            });
        });
    }

    // transport button: text only, works at any height
    fn transport_btn(
        &self, ui: &mut egui::Ui, ctx: &egui::Context,
        label: &str, w: f32, h: f32,
        enabled: bool, color: Color32, on_click: impl FnOnce(),
    ) {
        let p = &self.palette;
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(w, h), if enabled { Sense::click() } else { Sense::hover() });
        let hov = resp.hovered() && enabled;
        let bg = if !enabled { Color32::from_rgb(p.bg.r().saturating_add(4),
                                                  p.bg.g().saturating_add(4),
                                                  p.bg.b().saturating_add(5)) }
            else if hov { Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28) }
            else { p.surf2 };
        let border = if hov { color } else if enabled { p.bordbr } else { p.border };
        ui.painter().rect(rect, Rounding::same(6.0), bg, Stroke::new(1.0, border));
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label,
            FontId::monospace(9.0),
            if !enabled { Color32::from_rgba_unmultiplied(p.text.r(), p.text.g(), p.text.b(), 40) }
            else { color });
        if resp.clicked() && enabled { ctx.request_repaint(); on_click(); }
    }

    // -- Segment list ----------------------------------------------------------
    fn draw_segment_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (seg_count, is_playing, is_idle, total_dur, meta) = {
            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
            let sr  = rec.project.sample_rate;
            let ip  = rec.playback_state == PlaybackState::Playing;
            let ii  = matches!(rec.state, AppState::Idle);
            let td: f32 = rec.project.segments.iter().map(|s| s.duration_seconds(sr)).sum();
            // (index, sample_count, duration_secs, is_silence)
            let meta: Vec<(usize, usize, f32, bool)> = rec.project.segments.iter().enumerate()
                .map(|(i, s)| (i, s.samples.len(), s.duration_seconds(sr), s.is_silence))
                .collect();
            (rec.get_segment_count(), ip, ii, td, meta)
        }; 

        let p = &self.palette;

        ui.horizontal(|ui| {
            ui.label(RichText::new("SEGMENTS ").font(FontId::monospace(9.0)).color(p.dim).strong());
            if seg_count > 0 {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{} ", seg_count)).font(FontId::monospace(9.0)).color(p.mono));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let m = (total_dur / 60.0) as u32;
                    let s = (total_dur % 60.0) as u32;
                    ui.label(RichText::new(format!("{:02}:{:02} total ", m, s))
                        .font(FontId::monospace(9.0)).color(p.dim));
                });
            }
        });
        ui.add_space(6.0);

        if seg_count == 0 {
            egui::Frame::none()
                .fill(p.surf).rounding(Rounding::same(8.0))
                .stroke(Stroke::new(1.0, p.border))
                .inner_margin(egui::Margin::same(16.0))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("no segments yet  --  press RECORD to begin ")
                            .font(FontId::monospace(10.0))
                            .color(Color32::from_rgba_unmultiplied(p.text.r(), p.text.g(), p.text.b(), 38)));
                    });
                });
            return;
        }

        let available_height = ui.available_height();
        let reserved_height  = 90.0;
        let scroll_height    = (available_height - reserved_height).max(100.0);

        let mut preview_request: Option<(usize, SeekKind)> = None;

        egui::ScrollArea::vertical()
            .max_height(scroll_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (idx, n, dur, is_silence) in &meta {
                    let selected = self.selected_segment == Some(*idx);
                    let req = self.draw_segment_row(ui, ctx, *idx, *n, *dur, *is_silence,
                                                    is_playing, is_idle, selected);
                    if let Some(edge) = req { preview_request = Some(edge); }
                    // FIX: slightly more space between rows
                    ui.add_space(5.0); 
                }
            });

        if let Some((idx, kind)) = preview_request {
            match kind {
                SeekKind::EdgePreview(from_start) => {
                    let preview_secs = self.settings.trim_preview_secs;
                    // Compute the offset play_segment_edge will use so the
                    // progress bar starts from the same point.
                    self.playback_display_offset = if from_start {
                        0.0
                    } else {
                        // Re-read the (post-trim) duration for the from-end case.
                        let dur = {
                            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
                            rec.project.segments.get(idx)
                                .map(|s| s.samples.len() as f32 / rec.project.sample_rate as f32)
                                .unwrap_or(0.0)
                        };
                        (dur - preview_secs).max(0.0)
                    };
                    self.play_segment_edge(idx, from_start);
                }
                SeekKind::SeekTo(offset_secs) => {
                    self.playback_display_offset = offset_secs;
                    self.play_segment_from(idx, offset_secs, None);
                }
            }
        }
    }

    // -- Segment row -----------------------------------------------------------
    #[allow(clippy::too_many_arguments)]
    fn draw_segment_row(
        &mut self, ui: &mut egui::Ui, ctx: &egui::Context,
        idx: usize, samples: usize, duration: f32, is_silence: bool,
        is_playing: bool, is_idle: bool, is_selected: bool,
    ) -> Option<(usize, SeekKind)> {
        let p = &self.palette;

        // -- Layout constants --------------------------------------------------
        let row_w    = ui.available_width();
        
        let main_h   = 46.0_f32; 
        
        // expanded height is 78.0 to prevent internal elements from squashing downwards
        let trim_h   = if is_selected { 78.0_f32 } else { 0.0 }; 
        let total_h  = main_h + trim_h;
        
        let btn_w    = 52.0_f32; 
        let btn_h    = 28.0_f32; 
        let btn_gap  = 4.0_f32;  
        let n_btns   = if is_idle && !is_playing { if is_silence { 3 } else { 4 } } else { 0 };
        let btns_total = if n_btns > 0 { n_btns as f32 * btn_w + (n_btns - 1) as f32 * btn_gap + 8.0 } else { 0.0 };

        // -- allocate the whole row (advances layout cursor) -------------------
        let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_w, total_h), Sense::hover());

        // -- background & border -----------------------------------------------
        // Silence rows use a desaturated blue tint so they're immediately
        // distinguishable from recorded audio
        let silence_base = blend(p.bg, p.blue, 0.10);
        let bg = if is_silence {
            if is_selected { blend(silence_base, p.blue, 0.10) } else { silence_base }
        } else if is_selected { blend(p.surf, p.blue, 0.07) } else {
            p.surf
        };
        let border_col = if is_silence {
            if is_selected { blend(p.border, p.blue, 0.70) } else { blend(p.border, p.blue, 0.40) }
        } else if is_selected { blend(p.border, p.blue, 0.5) } else { p.border };

        ui.painter().rect(row_rect, Rounding::same(6.0), bg, Stroke::new(1.0, border_col));

        // -- info text ---------------------------------------------------------
        let cy = row_rect.min.y + main_h / 2.0;

        // Index number — silence rows rendered in blue, audio rows in mono (teal)
        let idx_col = if is_silence { p.blue } else { p.mono };
        ui.painter().text(Pos2::new(row_rect.min.x + 18.0, cy),
            egui::Align2::CENTER_CENTER,
            format!("{:02}", idx + 1), FontId::monospace(13.0), idx_col);

        let dm = (duration / 60.0) as u32;
        ui.painter().text(Pos2::new(row_rect.min.x + 52.0, cy),
            egui::Align2::LEFT_CENTER,
            format!("{:02}:{:04.1}", dm, duration % 60.0), FontId::monospace(12.0), p.text);

        if is_silence {
            // "SILENCE" badge replaces the sample count to make the type obvious at a glance.
            // Draw a small filled pill for the badge background, then the text on top.
            let badge_x = row_rect.min.x + 140.0;
            let badge_rect = Rect::from_center_size(
                Pos2::new(badge_x + 24.0, cy),
                Vec2::new(52.0, 14.0));
            ui.painter().rect_filled(badge_rect, Rounding::same(3.0),
                Color32::from_rgba_unmultiplied(p.blue.r(), p.blue.g(), p.blue.b(), 40));
            ui.painter().text(badge_rect.center(), egui::Align2::CENTER_CENTER,
                "SILENCE", FontId::monospace(7.5),
                blend(p.blue, p.text, 0.55));
        } else {
            ui.painter().text(Pos2::new(row_rect.min.x + 140.0, cy),
                egui::Align2::LEFT_CENTER,
                format!("{} smp", samples), FontId::monospace(9.0), p.dim);
        }

        // -- info zone click (expand/collapse trim panel) ----------------------
        let info_w    = (row_w - btns_total - 10.0).max(10.0);
        let info_rect = Rect::from_min_size(row_rect.min, Vec2::new(info_w, main_h));
        let info_resp = ui.interact(info_rect, ui.id().with(("info", idx)), Sense::click());
        if info_resp.hovered() && is_idle {
            ui.painter().rect_filled(info_rect, Rounding::same(5.0),
                Color32::from_rgba_unmultiplied(255, 255, 255, 5));
        }

        // -- action buttons (right side, explicit pixel positions) -------------
        let mut pending: Option<Command> = None;
        if is_idle && !is_playing {
            // Silence rows: INSERT  PLAY  DEL  (no RETRY silence isn't a take)
            // Normal rows:  INSERT  PLAY  RETRY  DEL
            let specs_normal: &[(&str, Color32, fn(usize) -> Command)] = &[
                ("INSERT", p.blue,  Command::InsertAfter   as fn(usize) -> Command),
                ("PLAY",   p.play,  Command::PlaySegment   as fn(usize) -> Command),
                ("RETRY",  p.amber, Command::RetrySegment  as fn(usize) -> Command),
                ("DEL",    p.rec,   Command::DeleteSegment as fn(usize) -> Command),
            ];
            let specs_silence: &[(&str, Color32, fn(usize) -> Command)] = &[
                ("INSERT", p.blue,  Command::InsertAfter   as fn(usize) -> Command),
                ("PLAY",   p.play,  Command::PlaySegment   as fn(usize) -> Command),
                ("DEL",    p.rec,   Command::DeleteSegment as fn(usize) -> Command),
            ];
            let specs = if is_silence { specs_silence } else { specs_normal };

            let start_x = row_rect.max.x - btns_total + 4.0;
            for (i, (lbl, col, cmd_fn)) in specs.iter().enumerate() {
                let x = start_x + i as f32 * (btn_w + btn_gap);
                let btn_rect = Rect::from_min_size(
                    Pos2::new(x, row_rect.min.y + (main_h - btn_h) / 2.0),
                    Vec2::new(btn_w, btn_h));
                let resp = ui.interact(btn_rect, ui.id().with(("btn", idx, i)), Sense::click());
                let h = resp.hovered();
                ui.painter().rect(btn_rect, Rounding::same(4.0),
                    if h { Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 35) } else { p.surf2 },
                    Stroke::new(1.0, if h { *col } else { p.border }));
                ui.painter().text(btn_rect.center(), egui::Align2::CENTER_CENTER,
                    lbl, FontId::monospace(7.5), if h { *col } else { p.dim });
                if resp.clicked() {
                    pending = Some(cmd_fn(idx));
                    ctx.request_repaint();
                }
            }
        }

        // -- toggle expand on info click (only if no button was clicked) -------
        if info_resp.clicked() && pending.is_none() && is_idle {
            if is_selected {
                self.selected_segment = None;
            } else {
                // expanding a new segment: reset the seek bar to the beginning
                self.seek_offset_secs  = 0.0;
                self.selected_segment  = Some(idx);
            }
        }

        // -- trim panel (shown when expanded) ----------------------------------
        // SeekKind::EdgePreview fires automatically after a trim (so the user
        // can hear the edit without clicking anything extra).
        // SeekKind::SeekTo fires when the user clicks / releases a drag on
        // the seek bar.  Both are bubbled up to draw_segment_list which
        // executes the right playback call once the scroll-area borrow is gone.
        let mut preview_edge: Option<SeekKind> = None;
        let mut stop_and_seek: Option<f32> = None;

        if is_selected {
            let sep_y = row_rect.min.y + main_h + 3.0;
            ui.painter().line_segment(
                [Pos2::new(row_rect.min.x + 8.0, sep_y), Pos2::new(row_rect.max.x - 8.0, sep_y)],
                Stroke::new(1.0, border_col));

            let panel_x   = row_rect.min.x + 8.0;
            let panel_w   = (row_w - 16.0).max(10.0);
            
            // spaced out internal components to utilize 78px height perfectly
            let row1_rect = Rect::from_min_size(Pos2::new(panel_x, sep_y + 6.0),  Vec2::new(panel_w, 28.0));
            let row2_rect = Rect::from_min_size(Pos2::new(panel_x, sep_y + 40.0), Vec2::new(panel_w, 28.0));

            if is_silence {
                // -- SILENCE expand panel ------------------------------------
                //
                // Row 1: EXPAND controls + TRIM controls side-by-side.
                //   Expanding adds more zeros to the end; trimming removes from
                //   either end both work identically to their audio counterparts.
                //
                // Row 2: Duration readout only 
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(row1_rect), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("EXPAND").font(FontId::monospace(8.0)).color(p.blue));
                        ui.add_space(4.0);
                        ui.add(egui::DragValue::new(&mut self.silence_secs)
                            .range(0.01_f32..=60.0).speed(0.01).suffix(" s").fixed_decimals(2));
                        ui.add_space(6.0);

                        let ss = self.silence_secs;
                        let can_act = ss > 0.0 && is_idle && !is_playing;

                        let (exp_r, exp_resp) = ui.allocate_exact_size(
                            Vec2::new(62.0, 20.0),
                            if can_act { Sense::click() } else { Sense::hover() });
                        let eh = exp_resp.hovered() && can_act;
                        ui.painter().rect(exp_r, Rounding::same(3.0),
                            if eh { Color32::from_rgba_unmultiplied(p.blue.r(), p.blue.g(), p.blue.b(), 35) } else { p.surf3 },
                            Stroke::new(1.0, if eh { p.blue } else { border_col }));
                        ui.painter().text(exp_r.center(), egui::Align2::CENTER_CENTER,
                            "expand +", FontId::monospace(7.5),
                            if can_act { p.blue } else { p.muted });
                        if exp_resp.clicked() && can_act {
                            pending = Some(Command::ExpandSilence(idx, ss));
                            ctx.request_repaint();
                        }

                        // -- Visual divider -----------------------------------
                        ui.add_space(10.0);
                        let divider_rect = ui.painter().clip_rect();
                        let dx = ui.next_widget_position().x;
                        ui.painter().line_segment(
                            [Pos2::new(dx, divider_rect.min.y + 4.0),
                             Pos2::new(dx, divider_rect.min.y + 20.0)],
                            Stroke::new(1.0, p.border));
                        ui.add_space(10.0);

                        // -- Trim section -------------------------------------
                        ui.label(RichText::new("TRIM").font(FontId::monospace(8.0)).color(p.dim));
                        ui.add_space(4.0);
                        ui.add(egui::DragValue::new(&mut self.trim_amount)
                            .range(0.0_f32..=60.0).speed(0.01).suffix(" s").fixed_decimals(2));
                        ui.add_space(8.0);

                        let ta = self.trim_amount;
                        let can_trim = ta > 0.0 && is_idle && !is_playing;
                        for (lbl, is_start) in [
                            ("< trim start", true),
                            ("trim end >",   false),
                        ] {
                            let (tr, tresp) = ui.allocate_exact_size(
                                Vec2::new(68.0, 20.0),
                                if can_trim { Sense::click() } else { Sense::hover() });
                            let th = tresp.hovered() && can_trim;
                            ui.painter().rect(tr, Rounding::same(3.0),
                                if th { Color32::from_rgba_unmultiplied(p.amber.r(), p.amber.g(), p.amber.b(), 35) } else { p.surf3 },
                                Stroke::new(1.0, if th { p.amber } else { p.border }));
                            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER,
                                lbl, FontId::monospace(7.5), if can_trim { p.amber } else { p.muted });
                            if tresp.clicked() && can_trim {
                                pending = Some(if is_start {
                                    Command::TrimStart(Some(idx), ta)
                                } else {
                                    Command::TrimEnd(Some(idx), ta)
                                });
                                ctx.request_repaint();
                            }
                            ui.add_space(4.0);
                        }
                    });
                });

                // Row 2: duration only
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(row2_rect), |ui| {
                    ui.horizontal(|ui| {
                        let dm = (duration / 60.0) as u32;
                        let ds = duration % 60.0;
                        ui.label(RichText::new(format!("dur  {:02}:{:05.2}", dm, ds))
                            .font(FontId::monospace(8.0)).color(p.dim));
                        ui.add_space(10.0);
                        ui.label(RichText::new("silence — no audio preview")
                            .font(FontId::monospace(7.5))
                            .color(Color32::from_rgba_unmultiplied(p.blue.r(), p.blue.g(), p.blue.b(), 120)));
                    });
                });

            } else {
                // -- Normal (audio) segment expand panel ----------------------
                //
                // Row 1: TRIM amount + trim start/end buttons  (unchanged)
                // Row 2: Duration + preview start/end + silence_secs DragValue + "+ silence after"
                //   The silence shortcut lets the user add a pause right after this
                //   take without scrolling away or using a separate command.

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(row1_rect), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("TRIM").font(FontId::monospace(8.0)).color(p.dim));
                        ui.add_space(10.0);
                        // trim point is driven by the seek bar below — no manual entry needed.
                        // ui.label(RichText::new("use seek bar to set trim point")
                        //     .font(FontId::monospace(7.5)).color(p.muted));
                        // ui.add_space(10.0);

                        let seek = self.seek_offset_secs;
                        // each button is independently guarded: trim-start needs the
                        // seek cursor to be past 0, trim-end needs it before the end.
                        let can_trim_start = seek > 0.0 && is_idle && !is_playing;
                        let can_trim_end   = seek < duration && is_idle && !is_playing;

                        for (lbl, is_start, can_trim) in [
                            ("< trim start", true,  can_trim_start),
                            ("trim end >",   false, can_trim_end),
                        ] {
                            let (tr, tresp) = ui.allocate_exact_size(
                                Vec2::new(76.0, 20.0),
                                if can_trim { Sense::click() } else { Sense::hover() });
                            let th = tresp.hovered() && can_trim;
                            ui.painter().rect(tr, Rounding::same(3.0),
                                if th { Color32::from_rgba_unmultiplied(p.amber.r(), p.amber.g(), p.amber.b(), 35) } else { p.surf3 },
                                Stroke::new(1.0, if th { p.amber } else { p.border }));
                            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER,
                                lbl, FontId::monospace(7.5), if can_trim { p.amber } else { p.muted });
                            if tresp.clicked() && can_trim {
                                pending = Some(if is_start {
                                    // Trim start: reset seek to beginning so the user
                                    // sees the new start and the edge preview plays from 0.
                                    self.seek_offset_secs = 0.0;
                                    Command::TrimStart(Some(idx), seek)
                                } else {
                                    // Trim end: park seek just before the new end so the
                                    // bar shows roughly where the edge preview will start.
                                    let preview_secs = self.settings.trim_preview_secs;
                                    self.seek_offset_secs = (seek - preview_secs).max(0.0);
                                    Command::TrimEnd(Some(idx), duration - seek)
                                });
                                // mark which edge was trimmed so we can auto-preview it
                                preview_edge = Some(SeekKind::EdgePreview(is_start));
                                ctx.request_repaint();
                            }
                            ui.add_space(4.0);
                        }
                    });
                });

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(row2_rect), |ui| {
                    ui.horizontal(|ui| {
                        // live duration display updates instantly after trim
                        let dm = (duration / 60.0) as u32;
                        let ds = duration % 60.0;
                        ui.label(RichText::new(format!("dur  {:02}:{:05.2}", dm, ds))
                            .font(FontId::monospace(8.0)).color(p.dim));

                        ui.add_space(10.0);

                        let can_preview = is_idle && !is_playing;
                        // The seek bar stays interactive even during playback for the
                        // currently-expanded segment so the user can re-seek without
                        // first pressing Stop.
                        let can_seek = can_preview
                            || (is_playing && self.selected_segment == Some(idx));

                        // -- seek bar -------------------------------------------------
                        // Width: fill everything between the duration label and the
                        // silence controls on the right (DragValue + button + gaps).
                        let silence_rhs = 6.0 + 54.0 + 4.0 + 84.0; // gaps + DragValue + button
                        let seek_w = (ui.available_width() - silence_rhs - 4.0).max(60.0);

                        let (seek_r, seek_resp) = ui.allocate_exact_size(
                            Vec2::new(seek_w, 18.0),
                            if can_seek { Sense::click_and_drag() } else { Sense::hover() });

                        // background track
                        let sh = seek_resp.hovered() && can_seek;
                        ui.painter().rect(seek_r, Rounding::same(3.0),
                            p.surf3,
                            Stroke::new(1.0, if sh { p.bordbr } else { p.border }));

                        // Waveform (drawn over bg, under fill & needle)
                        // Reads self.waveform_cache which holds pre-computed
                        // peaks from update_waveform_caches()
                        // skipped for silence segments (since all zeros, no useful shape)
                        if !is_silence {
                            if let Some((_, peaks)) = self.waveform_cache.get(&idx) {
                                let n = peaks.len();
                                if n > 0 {
                                    let bar_w = seek_r.width() / n as f32;
                                    let cy    = seek_r.center().y;
                                    // use slightly less than full half-height so there's
                                    // a thin margin at the top and bottom of the track
                                    let half_h = seek_r.height() * 0.43;
                                    for (i, &peak) in peaks.iter().enumerate() {
                                        let x = seek_r.min.x + i as f32 * bar_w;
                                        // same dB scaling as the live waveform
                                        let display = if peak < 1e-6 { 0.0_f32 } else {
                                            ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
                                        };
                                        let bar_h = (display * half_h * 2.0).max(1.0);
                                        let bar_rect = Rect::from_center_size(
                                            Pos2::new(x + bar_w * 0.5, cy),
                                            Vec2::new((bar_w - 0.5).max(0.5), bar_h),
                                        );
                                        // subtle teal at 65/255 alpha visible but doesn't
                                        // compete with the trim fill or the playhead needle
                                        ui.painter().rect_filled(
                                            bar_rect,
                                            Rounding::ZERO,
                                            Color32::from_rgba_unmultiplied(
                                                p.mono.r(), p.mono.g(), p.mono.b(), 65),
                                        );
                                    }
                                }
                            }
                        }

                        // filled region up to seek position (trim cursor)
                        let seek_frac = if duration > 0.0 {
                            (self.seek_offset_secs / duration).clamp(0.0, 1.0)
                        } else { 0.0 };
                        if seek_frac > 0.0 {
                            let fill_r = Rect::from_min_size(
                                seek_r.min,
                                Vec2::new(seek_r.width() * seek_frac, seek_r.height()));
                            ui.painter().rect_filled(fill_r, Rounding::same(3.0),
                                Color32::from_rgba_unmultiplied(
                                    p.play.r(), p.play.g(), p.play.b(), 38));
                        }

                        // playhead needle
                        let needle_x = seek_r.min.x + seek_r.width() * seek_frac;
                        ui.painter().line_segment(
                            [Pos2::new(needle_x, seek_r.min.y + 2.0),
                             Pos2::new(needle_x, seek_r.max.y - 2.0)],
                            Stroke::new(1.5, if can_seek { p.play } else { p.muted }));

                        // -- playback progress overlay -----------------------------
                        // Drawn on top of the trim cursor. Only visible for the
                        // currently-expanded segment while audio is playing.
                        // Uses wall-clock elapsed time + the start offset recorded
                        // when playback began — no audio-thread polling needed.
                        let playback_pos = if is_playing && self.selected_segment == Some(idx)
                            && duration > 0.0
                        {
                            let elapsed = (self.frame_time - self.playback_wall_start) as f32;
                            (self.playback_display_offset + elapsed).min(duration)
                        } else {
                            -1.0 // sentinel: don't draw
                        };

                        if playback_pos >= 0.0 {
                            let pb_frac = (playback_pos / duration).clamp(0.0, 1.0);
                            let pb_x = seek_r.min.x + seek_r.width() * pb_frac;

                            // Amber fill behind the playhead so it reads against any bg
                            let pb_fill = Rect::from_min_size(
                                seek_r.min,
                                Vec2::new(seek_r.width() * pb_frac, seek_r.height()));
                            ui.painter().rect_filled(pb_fill, Rounding::same(3.0),
                                Color32::from_rgba_unmultiplied(
                                    p.amber.r(), p.amber.g(), p.amber.b(), 22));

                            // Solid amber needle
                            ui.painter().line_segment(
                                [Pos2::new(pb_x, seek_r.min.y + 1.0),
                                 Pos2::new(pb_x, seek_r.max.y - 1.0)],
                                Stroke::new(2.0, p.amber));
                        }

                        // time label: show live playback position while playing,
                        // seek cursor position otherwise. Colour matches the needle.
                        let (display_secs, label_col) = if playback_pos >= 0.0 {
                            (playback_pos, p.amber)
                        } else {
                            (self.seek_offset_secs, if can_seek { p.dim } else { p.muted })
                        };
                        let lm = (display_secs / 60.0) as u32;
                        let ls = display_secs % 60.0;
                        ui.painter().text(
                            seek_r.center(), egui::Align2::CENTER_CENTER,
                            format!("{:02}:{:04.1}", lm, ls),
                            FontId::monospace(7.0),
                            label_col);

                        // drag: update seek position live (no playback until release)
                        // Only when not already playing dragging during playback
                        // would fight the live progress needle.
                        if seek_resp.dragged() && can_preview {
                            if let Some(pos) = seek_resp.interact_pointer_pos() {
                                let frac = ((pos.x - seek_r.min.x) / seek_r.width())
                                    .clamp(0.0, 1.0);
                                self.seek_offset_secs = frac * duration;
                                ctx.request_repaint();
                            }
                        }
                        // click or drag-release: start playback from the new position.
                        // If audio is already playing, stop it and stash a pending seek;
                        // update() will fire it the next frame once the thread goes Idle.
                        if (seek_resp.drag_stopped() || seek_resp.clicked()) && can_seek {
                            if let Some(pos) = seek_resp.interact_pointer_pos() {
                                let frac = ((pos.x - seek_r.min.x) / seek_r.width())
                                    .clamp(0.0, 1.0);
                                self.seek_offset_secs = frac * duration;
                            }
                            if is_playing {
                                // Can't start a new play call while Playing — queue it.
                                stop_and_seek = Some(self.seek_offset_secs);
                            } else {
                                preview_edge = Some(SeekKind::SeekTo(self.seek_offset_secs));
                            }
                            ctx.request_repaint();
                        }

                        // -- + silence after -----------------------------------
                        // Inline shortcut: insert a silence segment after this take.
                        // Uses self.silence_secs so the user can tune the default in
                        // one place and apply it across multiple segments.
                        ui.add_space(6.0);
                        ui.add(egui::DragValue::new(&mut self.silence_secs)
                            .range(0.01_f32..=60.0).speed(0.01).suffix(" s").fixed_decimals(2));
                        ui.add_space(4.0);

                        let ss = self.silence_secs;
                        let can_sil = ss > 0.0 && is_idle && !is_playing;
                        let (sr, sresp) = ui.allocate_exact_size(
                            Vec2::new(80.0, 18.0),
                            if can_sil { Sense::click() } else { Sense::hover() });
                        let sh = sresp.hovered() && can_sil;
                        ui.painter().rect(sr, Rounding::same(3.0),
                            if sh { Color32::from_rgba_unmultiplied(p.blue.r(), p.blue.g(), p.blue.b(), 28) } else { p.surf2 },
                            Stroke::new(1.0, if sh { p.blue } else { p.border }));
                        ui.painter().text(sr.center(), egui::Align2::CENTER_CENTER,
                            "+ silence after", FontId::monospace(7.0),
                            if can_sil { p.blue } else { p.muted });
                        if sresp.clicked() && can_sil {
                            pending = Some(Command::InsertSilenceAfter(idx, ss));
                            ctx.request_repaint();
                        }
                    });
                });
            }
        }

        // execute trim/expand/silence command first, then signal playback of the edge preview
        if let Some(cmd) = pending { self.handle_command(cmd); }

        // stop_and_seek is set when the user clicks the seek bar during playback.
        // self.handle_command can't be called inside the allocate_new_ui closure
        // (p borrows self.palette there), so we drain it here once all borrows are gone.
        if let Some(offset) = stop_and_seek {
            self.handle_command(Command::StopPlayback);
            self.pending_seek = Some((idx, offset));
        }

        // return the preview/seek request to the caller (draw_segment_list) so it can
        // be executed after the scroll area releases its borrow of self.
        if let Some(kind) = preview_edge {
            return Some((idx, kind));
        }

        None
    }

    // -- footer ----------------------------------------------------------------
    fn draw_footer(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = &self.palette;
        let (seg_count, is_idle, is_playing, current_save_path) = {
            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
            (
                rec.get_segment_count(),
                matches!(rec.state, AppState::Idle),
                rec.playback_state == PlaybackState::Playing,
                rec.save_path.clone() 
            )
        };

        // using horizontal_wrapped makes sure the buttons safely drop to a new line 
        // when the window is too small, rather than forcing the entire UI to stretch off-screen.
        ui.horizontal_wrapped(|ui| {
            // Apply standard spacing for wrapped items
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().item_spacing.y = 8.0;

            let can_interact = is_idle && !is_playing;
            let can_export = seg_count > 0 && can_interact;

            let footer_btn = |ui: &mut egui::Ui, label: &str, enabled: bool, width: f32| -> bool {
                let (rect, resp) = ui.allocate_exact_size(
                    Vec2::new(width, 30.0),
                    if enabled { Sense::click() } else { Sense::hover() }
                );
                let hov = resp.hovered() && enabled;
                ui.painter().rect(
                    rect, Rounding::same(6.0), p.surf2,
                    Stroke::new(1.0, if hov { p.text } else { p.border })
                );
                ui.painter().text(
                    rect.center(), egui::Align2::CENTER_CENTER, label,
                    FontId::monospace(9.5),
                    if enabled { p.text } else { p.muted }
                );
                resp.clicked() && enabled
            };

            if footer_btn(ui, "LOAD PROJECT", can_interact, 120.0) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Project File", &["bin"])
                    .pick_file()
                {
                    self.handle_command(Command::LoadProject(path.to_string_lossy().to_string()));
                    ctx.request_repaint();
                }
            }

            let can_save = can_interact && current_save_path.is_some();
            if footer_btn(ui, "SAVE", can_save, 70.0) {
                if let Some(ref path) = current_save_path {
                    self.handle_command(Command::SaveProjectAs(path.clone()));
                    ctx.request_repaint();
                }
            }

            if footer_btn(ui, "SAVE AS...", can_interact, 100.0) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Project File", &["bin"])
                    .save_file()
                {
                    self.handle_command(Command::SaveProjectAs(path.to_string_lossy().to_string()));
                    ctx.request_repaint();
                }
            }

            if footer_btn(ui, "EXPORT WAV", can_export, 110.0) {
                let mut dialog = rfd::FileDialog::new().add_filter("WAV Audio", &["wav"]);

                let start_dir = self.settings.default_export_dir.clone()
                    .or_else(|| {
                        current_save_path.as_ref().and_then(|p| {
                            std::path::Path::new(p).parent()
                                .map(|d| d.to_string_lossy().to_string())
                        })
                    });
                if let Some(dir) = start_dir {
                    dialog = dialog.set_directory(dir);
                }

                if let Some(path) = dialog.save_file() {
                    self.handle_command(Command::Export(Some(path.to_string_lossy().to_string())));
                    ctx.request_repaint();
                }
            }

            ui.add_space(4.0);
            ui.label(RichText::new("? for keybindings")
                .font(FontId::monospace(8.0))
                .color(Color32::from_rgba_unmultiplied(p.text.r(), p.text.g(), p.text.b(), 28)));
        });
    }

    // -- keybindings overlay ---------------------------------------------------
    fn draw_keybindings_overlay(&mut self, ctx: &egui::Context) {
        let p = &self.palette;
        egui::Area::new(egui::Id::new("kb_overlay"))
            .fixed_pos(Pos2::ZERO)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let screen = ctx.screen_rect();
                let (bg_rect, bg_resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(bg_rect, Rounding::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 190));
                if bg_resp.clicked() { self.show_keybindings = false; }

                let card_w = 440.0_f32;
                let card_h = (screen.height() - 60.0).min(480.0);
                let card   = Rect::from_center_size(screen.center(), Vec2::new(card_w, card_h));
                ui.painter().rect(card, Rounding::same(12.0), p.surf2, Stroke::new(1.0, p.bordbr));

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card.shrink(22.0)), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("KEYBINDINGS")
                            .font(FontId::monospace(12.0)).color(p.text).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::click());
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER,
                                "X", FontId::monospace(10.0), p.dim);
                            if resp.clicked() { self.show_keybindings = false; }
                        });
                    });
                    ui.add_space(6.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(4.0);

                    let keys: &[(&str, &str, Color32)] = &[
                        ("R",                     "Start recording",             p.rec),
                        ("S",                     "Stop recording",              p.muted),
                        ("Space",                 "Stop playback",               p.amber),
                        ("C",                     "Confirm / approve take",      p.play),
                        ("X",                     "Reject take",                 p.rec),
                        ("T",                     "Try again (re-record slot)",  p.amber),
                        ("P",                     "Play last segment / listen",  p.play),
                        ("Ctrl-Z",                "Undo",                        p.muted),
                        ("Ctrl-Shift-Z / Ctrl-Y", "Redo",                        p.muted),
                        ("?",                     "Toggle keybindings panel",    p.mono),
                        ("Esc",                   "Close any open panel",        p.dim),
                        ("click segment row",     "Expand / collapse panel",     p.blue),
                        ("hover segment row",     "Reveal play / retry / del",   p.dim),
                        ("+ silence after",       "Insert silence (in expand)",  p.blue),
                        ("expand +",              "Lengthen silence segment",    p.blue),
                    ];

                    let scroll_h = card_h - 100.0;
                    egui::ScrollArea::vertical()
                        .max_height(scroll_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (key, desc, col) in keys {
                                ui.horizontal(|ui| {
                                    let (kr, _) = ui.allocate_exact_size(Vec2::new(170.0, 20.0), Sense::hover());
                                    ui.painter().text(
                                        Pos2::new(kr.min.x, kr.center().y), egui::Align2::LEFT_CENTER,
                                        *key, FontId::monospace(9.5), *col);
                                    ui.painter().text(
                                        Pos2::new(kr.max.x + 6.0, kr.center().y), egui::Align2::LEFT_CENTER,
                                        *desc, FontId::monospace(9.0), p.dim);
                                });
                                ui.add_space(4.0);
                            }
                        });

                    ui.add_space(6.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(4.0);
                    ui.label(RichText::new("press ? or click outside to close")
                        .font(FontId::monospace(8.5))
                        .color(Color32::from_rgba_unmultiplied(p.text.r(), p.text.g(), p.text.b(), 50)));
                });
            });
    }

    // -- settings overlay ------------------------------------------------------
    fn draw_settings_overlay(&mut self, ctx: &egui::Context) {
        let p = &self.palette;
        egui::Area::new(egui::Id::new("settings_overlay"))
            .fixed_pos(Pos2::ZERO)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let screen = ctx.screen_rect();
                let (bg_rect, bg_resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(bg_rect, Rounding::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 200));
                if bg_resp.clicked() { self.show_settings = false; }

                let card_w = 520.0_f32;
                let card_h = (screen.height() - 60.0).min(540.0);
                let card   = Rect::from_center_size(screen.center(), Vec2::new(card_w, card_h));
                ui.painter().rect(card, Rounding::same(12.0), p.surf2, Stroke::new(1.0, p.bordbr));

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card.shrink(24.0)), |ui| {
                    // -- title bar
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("⚙  SETTINGS")
                            .font(FontId::monospace(12.0)).color(p.text).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::click());
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER,
                                "X", FontId::monospace(10.0), p.dim);
                            if resp.clicked() { self.show_settings = false; }
                        });
                    });
                    ui.add_space(6.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(10.0);

                    // ── section: THEME ─────────────────────────────────────────
                    ui.label(RichText::new("THEME")
                        .font(FontId::monospace(9.0)).color(p.dim).strong());
                    ui.add_space(8.0);

                    // Theme swatches — two rows: DARK (top) and LIGHT (bottom).
                    // The closure is scoped so its immutable borrow of self.theme
                    // is released before we write self.theme = t below.
                    let swatch_w   = 82.0_f32;
                    let swatch_h   = 62.0_f32;
                    let swatch_gap = 8.0_f32;

                    let (dark_click, light_click) = {
                        // helper: draw one row of swatches, return clicked theme if any
                        let draw_swatch_row = |ui: &mut egui::Ui, kinds: &[ThemeKind]| -> Option<ThemeKind> {
                            let mut chosen = None;
                            ui.horizontal(|ui| {
                                for kind in kinds {
                                    let tp        = palette_for(kind);
                                    let is_active = &self.theme == kind;

                                    let (sw, sw_resp) = ui.allocate_exact_size(
                                        Vec2::new(swatch_w, swatch_h), Sense::click());

                                    // background card
                                    ui.painter().rect(sw, Rounding::same(7.0), tp.surf,
                                        Stroke::new(if is_active { 2.0 } else { 1.0 },
                                            if is_active              { tp.play   }
                                            else if sw_resp.hovered() { tp.bordbr }
                                            else                      { tp.border }));

                                    // colour preview strips (rec / play / amber)
                                    let strip_h = 10.0_f32;
                                    let strip_y = sw.min.y + 10.0;
                                    let strip_w = (swatch_w - 16.0) / 3.0;
                                    for (i, &col) in [tp.rec, tp.play, tp.amber].iter().enumerate() {
                                        let sx = sw.min.x + 8.0 + i as f32 * (strip_w + 2.0);
                                        ui.painter().rect_filled(
                                            Rect::from_min_size(Pos2::new(sx, strip_y),
                                                Vec2::new(strip_w, strip_h)),
                                            Rounding::same(2.0), col);
                                    }

                                    // BG tone bar
                                    ui.painter().rect_filled(
                                        Rect::from_min_size(
                                            Pos2::new(sw.min.x + 8.0, strip_y + strip_h + 4.0),
                                            Vec2::new(swatch_w - 16.0, 7.0)),
                                        Rounding::same(2.0), tp.bg);

                                    // active indicator dot
                                    if is_active {
                                        ui.painter().circle_filled(
                                            Pos2::new(sw.max.x - 10.0, sw.min.y + 10.0),
                                            4.0, tp.play);
                                    }

                                    // label
                                    ui.painter().text(
                                        Pos2::new(sw.center().x, sw.max.y - 11.0),
                                        egui::Align2::CENTER_CENTER,
                                        kind.label(), FontId::monospace(7.5),
                                        if is_active { tp.text } else { tp.dim });

                                    if sw_resp.clicked() { chosen = Some(kind.clone()); }
                                    ui.add_space(swatch_gap);
                                }
                            });
                            chosen
                        };

                        // ── Dark row ───────────────────────────────────────────
                        ui.label(RichText::new("DARK")
                            .font(FontId::monospace(8.0)).color(p.dim).strong());
                        ui.add_space(4.0);
                        let dc = draw_swatch_row(ui, ThemeKind::dark_themes());
                        ui.add_space(8.0);

                        // ── Light row ──────────────────────────────────────────
                        ui.label(RichText::new("LIGHT")
                            .font(FontId::monospace(8.0)).color(p.dim).strong());
                        ui.add_space(4.0);
                        let lc = draw_swatch_row(ui, ThemeKind::light_themes());

                        (dc, lc)
                    }; // closure dropped here — borrow on self.theme released

                    // apply whichever swatch (if any) was clicked this frame
                    if let Some(t) = dark_click.or(light_click) {
                        self.theme = t;
                    }

                    ui.add_space(14.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(10.0);

                    // -- section: PLAYBACK --------------------------------------
                    ui.label(RichText::new("PLAYBACK")
                        .font(FontId::monospace(9.0)).color(p.dim).strong());
                    ui.add_space(8.0);

                    // auto-play on stop toggle
                    ui.horizontal(|ui| {
                        let apos_on  = self.settings.auto_play_on_stop;
                        let (chk_r, chk_resp) = ui.allocate_exact_size(
                            Vec2::new(14.0, 14.0), Sense::click());
                        ui.painter().rect(chk_r, Rounding::same(3.0),
                            if apos_on { Color32::from_rgba_unmultiplied(p.play.r(), p.play.g(), p.play.b(), 60) }
                            else { p.surf3 },
                            Stroke::new(1.0, if apos_on { p.play } else { p.bordbr }));
                        if apos_on {
                            // draw a simple checkmark
                            let c = chk_r.center();
                            ui.painter().line_segment(
                                [Pos2::new(c.x - 3.5, c.y), Pos2::new(c.x - 1.0, c.y + 3.0)],
                                Stroke::new(1.5, p.play));
                            ui.painter().line_segment(
                                [Pos2::new(c.x - 1.0, c.y + 3.0), Pos2::new(c.x + 4.0, c.y - 2.5)],
                                Stroke::new(1.5, p.play));
                        }
                        if chk_resp.clicked() {
                            self.settings.auto_play_on_stop = !apos_on;
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("Auto-play segment after stopping")
                            .font(FontId::monospace(9.5)).color(p.text));
                    });

                    ui.add_space(8.0);

                    // trim preview duration
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Trim preview duration")
                            .font(FontId::monospace(9.5)).color(p.text));
                        ui.add_space(10.0);
                        ui.add(egui::DragValue::new(&mut self.settings.trim_preview_secs)
                            .range(0.5_f32..=30.0).speed(0.1).suffix(" s").fixed_decimals(1));
                        ui.add_space(6.0);
                        ui.label(RichText::new("(seconds played when previewing a trim edge)")
                            .font(FontId::monospace(8.0)).color(p.dim));
                    });

                    ui.add_space(14.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(10.0);

                    // -- section: PATHS -----------------------------------------
                    ui.label(RichText::new("PATHS")
                        .font(FontId::monospace(9.0)).color(p.dim).strong());
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Default export directory")
                            .font(FontId::monospace(9.5)).color(p.text));
                        ui.add_space(8.0);

                        let dir_label = self.settings.default_export_dir.clone()
                            .unwrap_or_else(|| "(same as project)".to_string());
                        let max_chars = 28_usize;
                        let truncated = if dir_label.len() > max_chars {
                            format!("…{}", &dir_label[dir_label.len() - max_chars..])
                        } else { dir_label.clone() };

                        let (dir_r, _) = ui.allocate_exact_size(Vec2::new(200.0, 18.0), Sense::hover());
                        ui.painter().rect(dir_r, Rounding::same(3.0), p.surf3,
                            Stroke::new(1.0, p.border));
                        ui.painter().text(
                            Pos2::new(dir_r.min.x + 6.0, dir_r.center().y),
                            egui::Align2::LEFT_CENTER,
                            &truncated, FontId::monospace(7.5), p.dim);

                        ui.add_space(6.0);

                        let (btn_r, btn_resp) = ui.allocate_exact_size(
                            Vec2::new(68.0, 18.0), Sense::click());
                        let bh = btn_resp.hovered();
                        ui.painter().rect(btn_r, Rounding::same(3.0),
                            if bh { Color32::from_rgba_unmultiplied(p.blue.r(), p.blue.g(), p.blue.b(), 30) }
                            else { p.surf2 },
                            Stroke::new(1.0, if bh { p.blue } else { p.border }));
                        ui.painter().text(btn_r.center(), egui::Align2::CENTER_CENTER,
                            "CHANGE...", FontId::monospace(7.5), if bh { p.blue } else { p.dim });
                        if btn_resp.clicked() {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                self.settings.default_export_dir =
                                    Some(dir.to_string_lossy().to_string());
                            }
                        }

                        // clear button (only when a path is set)
                        if self.settings.default_export_dir.is_some() {
                            ui.add_space(4.0);
                            let (clr_r, clr_resp) = ui.allocate_exact_size(
                                Vec2::new(30.0, 18.0), Sense::click());
                            let ch = clr_resp.hovered();
                            ui.painter().rect(clr_r, Rounding::same(3.0),
                                if ch { Color32::from_rgba_unmultiplied(p.rec.r(), p.rec.g(), p.rec.b(), 30) }
                                else { p.surf2 },
                                Stroke::new(1.0, if ch { p.rec } else { p.border }));
                            ui.painter().text(clr_r.center(), egui::Align2::CENTER_CENTER,
                                "CLR", FontId::monospace(7.5), if ch { p.rec } else { p.dim });
                            if clr_resp.clicked() {
                                self.settings.default_export_dir = None;
                            }
                        }
                    });

                    ui.add_space(16.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(6.0);
                    ui.label(RichText::new("press Esc or click outside to close")
                        .font(FontId::monospace(8.5))
                        .color(Color32::from_rgba_unmultiplied(p.text.r(), p.text.g(), p.text.b(), 50)));
                });
            });
    }

    // -- keyboard shortcuts ----------------------------------------------------
    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        // extract all recorder state BEFORE entering ctx.input().
        //
        // DEADLOCK FIX - ctx.input() acquires egui's internal read-lock
        // the audio thread calls on_new_data() (ctx.request_repaint()) while
        // holding the recorder mutex, which also needs egui's lock
        // if holding the recorder mutex inside ctx.input():
        //   GUI thread: egui lock held -> waiting for recorder mutex
        //   Audio thread: recorder mutex held -> waiting for egui lock
        // -> deadlock. Reading recorder state first, then dropping the mutex
        // before ctx.input(), breaks the cycle entirely.
        let (state_str, playing, count) = {
            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
            let s = match &rec.state {
                AppState::Idle      => "idle",
                AppState::Recording => "recording",
                AppState::Reviewing => "reviewing",
            };
            (s, rec.playback_state == PlaybackState::Playing, rec.get_segment_count())
        }; // recorder mutex fully released here

        ctx.input(|i| {
            // no recorder access inside this closure no deadlock possible.
            let ctrl = i.modifiers.ctrl || i.modifiers.command;

            // Esc closes whichever overlay is open (settings takes priority)
            if i.key_pressed(egui::Key::Escape) {
                if self.show_settings    { self.show_settings    = false; return; }
                if self.show_keybindings { self.show_keybindings = false; return; }
            }

            // Block game keys while any overlay is visible
            if self.show_settings || self.show_keybindings {
                if i.key_pressed(egui::Key::Questionmark) {
                    self.show_keybindings = !self.show_keybindings;
                    self.show_settings = false;
                }
                return;
            }

            if i.key_pressed(egui::Key::Questionmark) {
                self.show_keybindings = !self.show_keybindings;
                self.show_settings = false;
            }

            if i.key_pressed(egui::Key::R) && !ctrl && state_str == "idle" && !playing {
                self.handle_command(Command::StartRecording);
            }
            // Space stops playback from anywhere — no state restriction needed
            // since StopPlayback is a no-op when nothing is playing
            if i.key_pressed(egui::Key::Space) && playing {
                self.handle_command(Command::StopPlayback);
            }
            if i.key_pressed(egui::Key::S) && state_str == "recording" {
                self.handle_command(Command::StopRecording);
            }
            if i.key_pressed(egui::Key::C) && state_str == "reviewing" && !playing {
                self.handle_command(Command::Approve);
            }
            if i.key_pressed(egui::Key::X) && state_str == "reviewing" && !playing {
                self.handle_command(Command::Reject);
            }
            if i.key_pressed(egui::Key::T) && state_str == "reviewing" && !playing {
                self.handle_command(Command::RetryCurrentTake);
            }
            if i.key_pressed(egui::Key::P) {
                if state_str == "reviewing" { self.play_current_segment(); }
                else if count > 0 && !playing {
                    self.handle_command(Command::PlaySegment(count - 1));
                }
            }
            if ctrl && i.key_pressed(egui::Key::Z) && !i.modifiers.shift
                && state_str == "idle" && !playing
            {
                self.handle_command(Command::Undo);
            }
            if ctrl && (i.key_pressed(egui::Key::Y)
                || (i.modifiers.shift && i.key_pressed(egui::Key::Z)))
                && state_str == "idle" && !playing
            {
                self.handle_command(Command::Redo);
            }
        });
    }
}

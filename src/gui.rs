use eframe::egui::{
    self, Color32, FontId, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2,
};
use crate::state::{AppState, PlaybackState, Command};
use crate::RecorderApp;

// -- Palette ------------------------------------------------------------------
const BG:     Color32 = Color32::from_rgb(11,  11,  15 );
const SURF:   Color32 = Color32::from_rgb(18,  18,  24 );
const SURF2:  Color32 = Color32::from_rgb(24,  24,  34 );
const SURF3:  Color32 = Color32::from_rgb(32,  32,  46 );
const BORDER: Color32 = Color32::from_rgb(40,  40,  58 );
const BORDBR: Color32 = Color32::from_rgb(60,  60,  84 );
const REC:    Color32 = Color32::from_rgb(229, 72,  77 );
const PLAY:   Color32 = Color32::from_rgb(46,  204, 143);
const AMBER:  Color32 = Color32::from_rgb(245, 166, 35 );
const BLUE:   Color32 = Color32::from_rgb(74,  144, 217);
const MUTED:  Color32 = Color32::from_rgb(72,  72,  100);
const TEXT:   Color32 = Color32::from_rgb(237, 236, 233);
const DIM:    Color32 = Color32::from_rgb(100, 98,  120);
const MONO:   Color32 = Color32::from_rgb(148, 226, 199);


// -- eframe::App ---------------------------------------------------------------
impl eframe::App for RecorderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        {
            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(rec.state, AppState::Recording)
                || rec.playback_state == PlaybackState::Playing
            {
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            }
        }
        self.apply_theme(ctx);
        self.handle_keyboard(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame { fill: BG, ..Default::default() })
            .show(ctx, |ui| {
                let avail    = ui.available_width();
                let side_pad = ((avail - 700.0) / 2.0).max(20.0);
                egui::Frame::none()
                    .inner_margin(egui::Margin { left: side_pad, right: side_pad, top: 18.0, bottom: 16.0 })
                    .show(ui, |ui| {
                        self.draw_header(ui, ctx);
                        ui.add_space(16.0);
                        self.draw_transport_card(ui, ctx);
                        ui.add_space(14.0);
                        self.draw_segment_list(ui, ctx);
                        ui.add_space(12.0);
                        self.draw_footer(ui, ctx);
                    });
                if self.show_keybindings {
                    self.draw_keybindings_overlay(ctx);
                }
            });
    }
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

// -- RecorderApp impl ----------------------------------------------------------
impl RecorderApp {

    fn apply_theme(&self, ctx: &egui::Context) {
        let mut v = ctx.style().visuals.clone();
        v.panel_fill                       = BG;
        v.window_fill                      = SURF;
        v.extreme_bg_color                 = SURF2;
        v.widgets.noninteractive.bg_fill   = SURF2;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, DIM);
        v.widgets.inactive.bg_fill         = SURF2;
        v.widgets.inactive.fg_stroke       = Stroke::new(1.0, TEXT);
        v.widgets.hovered.bg_fill          = SURF3;
        v.widgets.active.bg_fill           = SURF3;
        v.selection.bg_fill                = Color32::from_rgba_unmultiplied(46, 204, 143, 40);
        v.override_text_color              = Some(TEXT);
        ctx.set_visuals(v);
    }

    // -- Header ----------------------------------------------------------------
    fn draw_header(&self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("PARTS").font(FontId::monospace(14.0)).color(TEXT).strong());
            ui.add_space(3.0);
            ui.label(RichText::new("OF").font(FontId::monospace(14.0)).color(DIM));
            ui.add_space(3.0);
            ui.label(RichText::new("SPEECH").font(FontId::monospace(14.0)).color(REC).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("? help").font(FontId::monospace(9.0))
                    .color(Color32::from_rgb(44, 44, 62)));
                ui.add_space(10.0);
                self.draw_status_badge(ui);
            });
        });
        ui.add_space(10.0);
        let r = ui.available_rect_before_wrap();
        ui.painter().line_segment([r.min, Pos2::new(r.max.x, r.min.y)], Stroke::new(1.0, BORDER));
        ui.add_space(1.0);
    }

    fn draw_status_badge(&self, ui: &mut egui::Ui) {
        // read time BEFORE locking the recorder. ui.input() internally
        // acquires egui's context read-lock; holding our recorder mutex
        // at the same time creates a lock-ordering inversion with egui's
        // repaint machinery and causes intermittent deadlocks.
        let t = ui.input(|i| i.time) as f32;
        let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
        let (text, col) = match (&rec.state, &rec.playback_state) {
            (AppState::Recording, _)    => ("REC",    REC),
            (_, PlaybackState::Playing) => ("PLAY",   PLAY),
            (AppState::Reviewing, _)    => ("REVIEW", AMBER),
            _                           => ("IDLE",   MUTED),
        };
        let alpha = if matches!(rec.state, AppState::Recording) {
            ((t * 2.8).sin() * 0.42 + 0.58).clamp(0.0, 1.0)
        } else { 1.0 };
        let col = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), (alpha * 255.0) as u8);
        ui.label(RichText::new(text).font(FontId::monospace(11.0)).color(col).strong());
    }

    // -- Transport Card --------------------------------------------------------
    fn draw_transport_card(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

        egui::Frame {
            fill: SURF, rounding: Rounding::same(10.0),
            stroke: Stroke::new(1.0, BORDER),
            inner_margin: egui::Margin { left: 20.0, right: 20.0, top: 16.0, bottom: 16.0 },
            ..Default::default()
        }
        .show(ui, |ui| {
            // -- Timer ---------------------------------------------------------
            let secs = cur_samples as f32 / sample_rate.max(1) as f32;
            let timer_col = match state_str {
                "recording" => REC, "reviewing" => AMBER,
                _ => Color32::from_rgb(36, 36, 54),
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
                ui.label(RichText::new(sub).font(FontId::monospace(10.0)).color(DIM));
            });

            ui.add_space(16.0);

            // ── Primary row: RECORD  STOP  PLAY  CONFIRM  REJECT ─────────────
            // width is computed at the START of the horizontal closure so
            // available_width() reflects the true inner width before any
            // allocations are made
            ui.horizontal(|ui| {
                let gap = 8.0_f32;
                let w   = ((ui.available_width() - gap * 4.0) / 5.0).max(1.0);
                let h   = 48.0_f32;

                self.transport_btn(ui, ctx, "RECORD", w, h,
                    state_str == "idle" && !is_playing, REC,
                    || self.handle_command(Command::StartRecording));
                ui.add_space(gap);
                self.transport_btn(ui, ctx, "STOP", w, h,
                    state_str == "recording", MUTED,
                    || self.handle_command(Command::StopRecording));
                ui.add_space(gap);
                let listen_lbl = if state_str == "reviewing" { "LISTEN" } else { "PLAY" };
                self.transport_btn(ui, ctx, listen_lbl, w, h,
                    !is_playing && (state_str == "reviewing" || seg_count > 0), PLAY,
                    || {
                        if state_str == "reviewing" { self.play_current_segment(); }
                        else if seg_count > 0 { self.handle_command(Command::PlaySegment(seg_count - 1)); }
                    });
                ui.add_space(gap);
                self.transport_btn(ui, ctx, "CONFIRM", w, h,
                    state_str == "reviewing" && !is_playing, PLAY,
                    || self.handle_command(Command::Approve));
                ui.add_space(gap);
                self.transport_btn(ui, ctx, "REJECT", w, h,
                    state_str == "reviewing" && !is_playing, REC,
                    || self.handle_command(Command::Reject));
            });

            ui.add_space(8.0);

            // -- Secondary row: TRY AGAIN  PLAY ALL  UNDO  REDO ---------------
            ui.horizontal(|ui| {
                let gap = 8.0_f32;
                let w   = ((ui.available_width() - gap * 3.0) / 4.0).max(1.0);
                let h   = 34.0_f32;

                self.transport_btn(ui, ctx, "TRY AGAIN", w, h,
                    state_str == "reviewing" && !is_playing, MUTED,
                    || self.handle_command(Command::RetryCurrentTake));
                ui.add_space(gap);
                self.transport_btn(ui, ctx, "PLAY ALL", w, h,
                    seg_count > 0 && !is_playing && state_str == "idle", MUTED,
                    || self.handle_command(Command::PlayAll));
                ui.add_space(gap);
                self.transport_btn(ui, ctx, "<< UNDO", w, h,
                    can_undo && state_str == "idle" && !is_playing, MUTED,
                    || self.handle_command(Command::Undo));
                ui.add_space(gap);
                self.transport_btn(ui, ctx, "REDO >>", w, h,
                    can_redo && state_str == "idle" && !is_playing, MUTED,
                    || self.handle_command(Command::Redo));
            });
        });
    }

    // transport button text only (no separate icon), works at any height
    fn transport_btn(
        &self, ui: &mut egui::Ui, ctx: &egui::Context,
        label: &str, w: f32, h: f32,
        enabled: bool, color: Color32, on_click: impl FnOnce(),
    ) {
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(w, h), if enabled { Sense::click() } else { Sense::hover() });
        let hov = resp.hovered() && enabled;
        let bg = if !enabled { Color32::from_rgb(15, 15, 20) }
            else if hov { Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28) }
            else { SURF2 };
        let border = if hov { color } else if enabled { BORDBR } else { BORDER };
        ui.painter().rect(rect, Rounding::same(6.0), bg, Stroke::new(1.0, border));
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label,
            FontId::monospace(9.0),
            if !enabled { Color32::from_rgb(38, 38, 55) } else { color });
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
            let meta: Vec<(usize, usize, f32)> = rec.project.segments.iter().enumerate()
                .map(|(i, s)| (i, s.samples.len(), s.duration_seconds(sr)))
                .collect();
            (rec.get_segment_count(), ip, ii, td, meta)
        }; // <-  mutex released here, drawing happens with no lock held
        ui.horizontal(|ui| {
            ui.label(RichText::new("SEGMENTS").font(FontId::monospace(9.0)).color(DIM).strong());
            if seg_count > 0 {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{}", seg_count)).font(FontId::monospace(9.0)).color(MONO));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let m = (total_dur / 60.0) as u32;
                    let s = (total_dur % 60.0) as u32;
                    ui.label(RichText::new(format!("{:02}:{:02} total", m, s))
                        .font(FontId::monospace(9.0)).color(DIM));
                });
            }
        });
        ui.add_space(6.0);

        if seg_count == 0 {
            egui::Frame::none()
                .fill(SURF).rounding(Rounding::same(8.0))
                .stroke(Stroke::new(1.0, BORDER))
                .inner_margin(egui::Margin::same(16.0))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("no segments yet  --  press RECORD to begin")
                            .font(FontId::monospace(10.0)).color(Color32::from_rgb(44, 44, 62)));
                    });
                });
            return;
        }

        egui::ScrollArea::vertical()
            .max_height(220.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (idx, n, dur) in &meta {
                    let selected = self.selected_segment == Some(*idx);
                    self.draw_segment_row(ui, ctx, *idx, *n, *dur, is_playing, is_idle, selected);
                    ui.add_space(3.0);
                }
            });
    }

    // -- Segment row -----------------------------------------------------------
    //
    // use ONE allocate_exact_size for the full row rect (advances the
    // layout cursor), then ui.interact(sub_rect, unique_id, sense) for every
    // interactive element. ui.interact does not advance the layout cursor, so
    // multiple sub-regions can coexist without fighting over the same space.
    //
    // Info zone (left side) and action buttons (right side) are given
    // non-overlapping rects by computing the buttons' total width first and
    // sizing the info zone to stop before them. This guarantees clicking a
    // button never also triggers the expand-toggle, regardless of window width.
    #[allow(clippy::too_many_arguments)]
    fn draw_segment_row(
        &mut self, ui: &mut egui::Ui, ctx: &egui::Context,
        idx: usize, samples: usize, duration: f32,
        is_playing: bool, is_idle: bool, is_selected: bool,
    ) {
        // -- Layout constants --------------------------------------------------
        let row_w    = ui.available_width();
        let main_h   = 42.0_f32;
        let trim_h   = 36.0_f32;  // extra height when expanded
        let total_h  = main_h + if is_selected { trim_h } else { 0.0 };
        let btn_w    = 50.0_f32;
        let btn_h    = 26.0_f32;
        let btn_gap  = 3.0_f32;
        let n_btns   = if is_idle && !is_playing { 4 } else { 0 };
        let btns_total = if n_btns > 0 { n_btns as f32 * btn_w + (n_btns - 1) as f32 * btn_gap + 8.0 } else { 0.0 };

        // -- allocate the whole row (advances layout cursor) -------------------
        let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_w, total_h), Sense::hover());

        // -- background & border -----------------------------------------------
        let bg = if is_selected { blend(SURF, BLUE, 0.07) } else { Color32::from_rgb(16, 16, 22) };
        let border_col = if is_selected { blend(BORDER, BLUE, 0.5) } else { BORDER };
        ui.painter().rect(row_rect, Rounding::same(6.0), bg, Stroke::new(1.0, border_col));

        // -- info text ---------------------------------------------------------
        let cy = row_rect.min.y + main_h / 2.0;
        ui.painter().text(Pos2::new(row_rect.min.x + 18.0, cy),
            egui::Align2::CENTER_CENTER,
            format!("{:02}", idx + 1), FontId::monospace(13.0), MONO);
        let dm = (duration / 60.0) as u32;
        ui.painter().text(Pos2::new(row_rect.min.x + 52.0, cy),
            egui::Align2::LEFT_CENTER,
            format!("{:02}:{:04.1}", dm, duration % 60.0), FontId::monospace(12.0), TEXT);
        ui.painter().text(Pos2::new(row_rect.min.x + 140.0, cy),
            egui::Align2::LEFT_CENTER,
            format!("{} smp", samples), FontId::monospace(9.0), DIM);

        // -- info zone click (expand/collapse trim panel) ----------------
        // width stops before the buttons so there is zero overlap.
        let info_w    = (row_w - btns_total - 10.0).max(10.0);
        let info_rect = Rect::from_min_size(row_rect.min, Vec2::new(info_w, main_h));
        let info_resp = ui.interact(info_rect, ui.id().with(("info", idx)), Sense::click());
        if info_resp.hovered() && is_idle {
            ui.painter().rect_filled(info_rect, Rounding::same(5.0),
                Color32::from_rgba_unmultiplied(255, 255, 255, 5));
        }

        // -- action buttons (right side, explicit pixel positions) -------------
        // by computing their rects from the right edge and using ui.interact
        // (not allocate_exact_size), they are completely independent from the
        // info zone above.
        let mut pending: Option<Command> = None;
        if is_idle && !is_playing {
            // left-to-right order: INSERT  PLAY  RETRY  DEL (rightmost = most destructive last)
            let specs: &[(&str, Color32, fn(usize) -> Command)] = &[
                ("INSERT", BLUE,  Command::InsertAfter   as fn(usize) -> Command),
                ("PLAY",   PLAY,  Command::PlaySegment   as fn(usize) -> Command),
                ("RETRY",  AMBER, Command::RetrySegment  as fn(usize) -> Command),
                ("DEL",    REC,   Command::DeleteSegment as fn(usize) -> Command),
            ];
            let start_x = row_rect.max.x - btns_total + 4.0;
            for (i, (lbl, col, cmd_fn)) in specs.iter().enumerate() {
                let x = start_x + i as f32 * (btn_w + btn_gap);
                let btn_rect = Rect::from_min_size(
                    Pos2::new(x, row_rect.min.y + (main_h - btn_h) / 2.0),
                    Vec2::new(btn_w, btn_h));
                let resp = ui.interact(btn_rect, ui.id().with(("btn", idx, i)), Sense::click());
                let h = resp.hovered();
                ui.painter().rect(btn_rect, Rounding::same(4.0),
                    if h { Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 35) } else { SURF2 },
                    Stroke::new(1.0, if h { *col } else { BORDER }));
                ui.painter().text(btn_rect.center(), egui::Align2::CENTER_CENTER,
                    lbl, FontId::monospace(7.5), if h { *col } else { DIM });
                if resp.clicked() {
                    pending = Some(cmd_fn(idx));
                    ctx.request_repaint();
                }
            }
        }

        // -- toggle expand on info click (only if no button was clicked) -------
        if info_resp.clicked() && pending.is_none() && is_idle {
            self.selected_segment = if is_selected { None } else { Some(idx) };
        }

        // -- trim panel (shown when expanded) ----------------------------------
        if is_selected {
            let sep_y = row_rect.min.y + main_h + 3.0;
            ui.painter().line_segment(
                [Pos2::new(row_rect.min.x + 8.0, sep_y), Pos2::new(row_rect.max.x - 8.0, sep_y)],
                Stroke::new(1.0, BORDER));

            let trim_rect = Rect::from_min_size(
                Pos2::new(row_rect.min.x + 8.0, sep_y + 5.0),
                Vec2::new((row_w - 16.0).max(10.0), 26.0));

            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(trim_rect), |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TRIM").font(FontId::monospace(8.0)).color(DIM));
                    ui.add_space(6.0);
                    ui.add(egui::DragValue::new(&mut self.trim_amount)
                        .range(0.0_f32..=60.0).speed(0.01).suffix(" s").fixed_decimals(2));
                    ui.add_space(10.0);
                    let ta = self.trim_amount;
                    let can_trim = ta > 0.0 && is_idle && !is_playing;
                    for (lbl, col, is_start) in [
                        ("< trim start", AMBER, true),
                        ("trim end >",   AMBER, false),
                    ] {
                        let (tr, tresp) = ui.allocate_exact_size(
                            Vec2::new(76.0, 20.0),
                            if can_trim { Sense::click() } else { Sense::hover() });
                        let th = tresp.hovered() && can_trim;
                        ui.painter().rect(tr, Rounding::same(3.0),
                            if th { Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 35) } else { SURF3 },
                            Stroke::new(1.0, if th { col } else { BORDER }));
                        ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER,
                            lbl, FontId::monospace(7.5), if can_trim { col } else { MUTED });
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
        }

        // execute any pending command 
        if let Some(cmd) = pending { self.handle_command(cmd); }
    }

    // -- footer ----------------------------------------------------------------
    fn draw_footer(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (seg_count, is_idle, is_playing) = {
            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
            (rec.get_segment_count(), matches!(rec.state, AppState::Idle),
             rec.playback_state == PlaybackState::Playing)
        };
        ui.horizontal(|ui| {
            let can_export = seg_count > 0 && is_idle && !is_playing;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(136.0, 30.0),
                if can_export { Sense::click() } else { Sense::hover() });
            let hov = resp.hovered() && can_export;
            ui.painter().rect(rect, Rounding::same(6.0), SURF2,
                Stroke::new(1.0, if hov { TEXT } else { BORDER }));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER,
                "EXPORT WAV", FontId::monospace(9.5), if can_export { TEXT } else { MUTED });
            if resp.clicked() && can_export {
                ctx.request_repaint();
                self.handle_command(Command::Export("output.wav".into()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("? for keybindings")
                    .font(FontId::monospace(8.0)).color(Color32::from_rgb(38, 38, 56)));
            });
        });
    }

    // -- keybindings overlay ---------------------------------------------------
    fn draw_keybindings_overlay(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("kb_overlay"))
            .fixed_pos(Pos2::ZERO)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let screen = ctx.screen_rect();
                let (bg_rect, bg_resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(bg_rect, Rounding::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 190));
                if bg_resp.clicked() { self.show_keybindings = false; }

                // card dimensions height capped to leave 60px margin top/bottom
                let card_w = 440.0_f32;
                let card_h = (screen.height() - 60.0).min(480.0);
                let card   = Rect::from_center_size(screen.center(), Vec2::new(card_w, card_h));
                ui.painter().rect(card, Rounding::same(12.0), SURF2, Stroke::new(1.0, BORDBR));

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card.shrink(22.0)), |ui| {
                    // title bar
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("KEYBINDINGS")
                            .font(FontId::monospace(12.0)).color(TEXT).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::click());
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER,
                                "X", FontId::monospace(10.0), DIM);
                            if resp.clicked() { self.show_keybindings = false; }
                        });
                    });
                    ui.add_space(6.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(4.0);

                    let keys: &[(&str, &str, Color32)] = &[
                        ("R",                     "Start recording",             REC),
                        ("S",                     "Stop recording",              MUTED),
                        ("C",                     "Confirm / approve take",      PLAY),
                        ("X",                     "Reject take",                 REC),
                        ("T",                     "Try again (re-record slot)",  AMBER),
                        ("P",                     "Play last segment / listen",  PLAY),
                        ("Ctrl-Z",                "Undo",                        MUTED),
                        ("Ctrl-Shift-Z / Ctrl-Y", "Redo",                        MUTED),
                        ("?",                     "Toggle this help panel",      MONO),
                        ("Esc",                   "Close this panel",            DIM),
                        ("click segment row",     "Expand / collapse trim",      BLUE),
                        ("hover segment row",     "Reveal play / retry / del",   DIM),
                    ];

                    // scrollArea ensures content never clips the card border
                    // regardless of window size or number of entries
                    let scroll_h = card_h - 100.0; // reserve space for title + footer
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
                                        *desc, FontId::monospace(9.0), DIM);
                                });
                                ui.add_space(4.0);
                            }
                        });

                    ui.add_space(6.0);
                    ui.add(egui::Separator::default());
                    ui.add_space(4.0);
                    ui.label(RichText::new("press ? or click outside to close")
                        .font(FontId::monospace(8.5)).color(Color32::from_rgb(50, 50, 68)));
                });
            });
    }

    // -- keyboard shortcuts ----------------------------------------------------
    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            let rec = self.recorder.lock().unwrap_or_else(|e| e.into_inner());
            let state_str = match &rec.state {
                AppState::Idle      => "idle",
                AppState::Recording => "recording",
                AppState::Reviewing => "reviewing",
            };
            let playing = rec.playback_state == PlaybackState::Playing;
            let count   = rec.get_segment_count();
            drop(rec);

            let ctrl = i.modifiers.ctrl || i.modifiers.command;

            if i.key_pressed(egui::Key::Questionmark) {
                self.show_keybindings = !self.show_keybindings;
            }
            if i.key_pressed(egui::Key::Escape) && self.show_keybindings {
                self.show_keybindings = false;
            }
            if self.show_keybindings { return; }

            if i.key_pressed(egui::Key::R) && !ctrl && state_str == "idle" && !playing {
                self.handle_command(Command::StartRecording);
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

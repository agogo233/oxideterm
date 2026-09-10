use super::*;

const SFTP_FIND_INPUT_WIDTH: f32 = 150.0;
const SFTP_FIND_INPUT_HEIGHT: f32 = 28.0;
const SFTP_FIND_BUTTON_SIZE: f32 = 28.0;
const SFTP_FIND_ICON_SIZE: f32 = 14.0;
const SFTP_FIND_MATCH_WIDTH: f32 = 48.0;

impl WorkspaceApp {
    pub(in crate::workspace::sftp) fn render_sftp_preview_body(
        &self,
        _has_background: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.tokens.ui;
        let (preview_loading, preview_error, preview_content) = {
            let sftp_view = self.sftp_view.read(cx);
            (
                sftp_view.preview_loading,
                sftp_view.preview_error.clone(),
                sftp_view.preview_content.clone(),
            )
        };
        let body = if preview_loading {
            self.render_sftp_preview_text(self.i18n.t("sftp.preview.loading"))
        } else if let Some(error) = preview_error {
            self.render_sftp_preview_text(error)
        } else if let Some(content) = preview_content.as_deref() {
            self.render_sftp_preview_content(content, cx)
        } else {
            self.render_sftp_preview_text(String::new())
        };
        let uses_virtual_text = self.sftp_preview_uses_virtual_text(cx);
        div()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .bg(rgb(theme.bg_sunken))
            .child(
                div()
                    .id("sftp-preview-scroll")
                    .flex_1()
                    .when(!uses_virtual_text, |scroll| {
                        scroll
                            .selectable_overflow_y_scroll(
                                &self.sftp_view.read(cx).preview_document_scroll,
                            )
                            .p(px(16.0))
                    })
                    .text_color(rgb(theme.text))
                    .child(body),
            )
            .into_any_element()
    }

    pub(in crate::workspace::sftp) fn render_sftp_editor_body(
        &self,
        _has_background: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.tokens.ui;
        let (language, encoding, editor, editor_saving, editor_dirty, editor_last_atomic_write) = {
            let sftp_view = self.sftp_view.read(cx);
            (
                sftp_view
                    .preview_editor_language
                    .clone()
                    .unwrap_or_else(|| "text".to_string()),
                sftp_view.preview_editor_encoding.clone(),
                sftp_view.preview_editor.clone(),
                sftp_view.preview_editor_saving,
                sftp_view.preview_editor_dirty,
                sftp_view.preview_editor_last_atomic_write,
            )
        };
        let (line, column) = editor
            .as_ref()
            .and_then(|editor| {
                let editor = editor.read(cx);
                editor
                    .buffer()
                    .offset_to_line_col(editor.cursor().selection().head)
                    .ok()
                    .map(|pos| (pos.line + 1, pos.column + 1))
            })
            .unwrap_or((1, 1));
        let status = if editor_saving {
            Some((self.i18n.t("sftp.preview.saving"), rgb(theme.text_muted)))
        } else if editor_dirty {
            Some((self.i18n.t("sftp.preview.modified"), rgb(SFTP_YELLOW)))
        } else if let Some(atomic) = editor_last_atomic_write {
            let key = if atomic {
                "sftp.preview.saved_atomic"
            } else {
                "sftp.preview.saved_direct"
            };
            Some((self.i18n.t(key), rgb(SFTP_GREEN)))
        } else {
            None
        };

        let preview_find_open = self.sftp_view.read(cx).preview_find_open;
        let find_bar = preview_find_open.then(|| self.render_sftp_preview_find_bar(cx));
        div()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .bg(rgb(theme.bg_sunken))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .when_some(editor.clone(), |body, editor| body.child(editor))
                    .when(editor.is_none(), |body| {
                        body.child(self.render_sftp_preview_text(String::new()))
                    })
                    .when_some(find_bar, |body, bar| body.child(bar)),
            )
            .child(
                div()
                    .h(px(32.0))
                    .flex_none()
                    .px(px(16.0))
                    .border_t_1()
                    .border_color(rgb(theme.border))
                    .bg(rgb(theme.bg_panel))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(SFTP_TEXT_XS))
                    .text_color(rgb(theme.text_muted))
                    .child(
                        div()
                            .flex()
                            .gap(px(16.0))
                            .child(self.render_selectable_text_scoped(
                                "sftp-editor-cursor-position",
                                (),
                                format!(
                                    "{} {}, {} {}",
                                    self.i18n.t("sftp.preview.line"),
                                    line,
                                    self.i18n.t("sftp.preview.column"),
                                    column
                                ),
                                theme.text_muted,
                                cx,
                            ))
                            .child(self.render_selectable_text_scoped(
                                "sftp-editor-language",
                                (),
                                language,
                                theme.text_muted,
                                cx,
                            ))
                            .child(self.render_selectable_text_scoped(
                                "sftp-editor-encoding",
                                (),
                                format!("{} {}", self.i18n.t("sftp.preview.encoding"), encoding),
                                theme.text_muted,
                                cx,
                            )),
                    )
                    .child(self.render_sftp_editor_status(status, cx)),
            )
            .into_any_element()
    }

    // Only find is supported here; the document keeps its caret while the query
    // owns platform text input, so the bar must not focus the editor itself.
    fn render_sftp_preview_find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.tokens.ui;
        let (query, find_case, focused, match_label, can_step) = {
            let sftp_view = self.sftp_view.read(cx);
            let query = sftp_view.input_value(SftpInput::PreviewFind).to_string();
            let query_empty = query.is_empty();
            let (match_label, can_step) = sftp_view
                .preview_editor
                .as_ref()
                .map(|editor| {
                    let editor = editor.read(cx);
                    let total = editor.find_matches().len();
                    let current = editor
                        .active_find_position()
                        .map(|(current, _)| current)
                        .unwrap_or(0);
                    (
                        if query_empty {
                            String::new()
                        } else if total > 999 {
                            "999+".to_string()
                        } else {
                            format!("{current}/{total}")
                        },
                        total > 0,
                    )
                })
                .unwrap_or((String::new(), false));
            (
                query,
                sftp_view.preview_find_case,
                sftp_view.focused_input == Some(SftpInput::PreviewFind),
                match_label,
                can_step,
            )
        };

        div()
            .absolute()
            .top(px(8.0))
            .right(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .p(px(4.0))
            .rounded(px(self.tokens.radii.sm))
            .border_1()
            .border_color(rgb(theme.border))
            .bg(rgb(theme.bg_elevated))
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation()
            })
            .child(
                div()
                    .relative()
                    .w(px(SFTP_FIND_INPUT_WIDTH))
                    .h(px(SFTP_FIND_INPUT_HEIGHT))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(8.0))
                    .rounded(px(self.tokens.radii.sm))
                    .border_1()
                    .border_color(if focused {
                        rgb(theme.accent)
                    } else {
                        rgb(theme.border)
                    })
                    .bg(rgb(theme.bg))
                    .child(Self::render_lucide_icon(
                        LucideIcon::Search,
                        SFTP_FIND_ICON_SIZE,
                        rgb(theme.text_muted),
                    ))
                    .child(self.render_sftp_inline_text(
                        SftpInput::PreviewFind,
                        None,
                        &query,
                        "ide.find_placeholder",
                        focused,
                        cx,
                    )),
            )
            .child(
                div()
                    .size(px(SFTP_FIND_BUTTON_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.tokens.radii.sm))
                    .bg(if find_case {
                        rgba((theme.accent << 8) | SFTP_SELECTED_BG_ALPHA)
                    } else {
                        rgba((theme.bg_hover << 8) | SFTP_BUTTON_TRANSPARENT_ALPHA)
                    })
                    .text_color(rgb(if find_case {
                        theme.accent
                    } else {
                        theme.text_muted
                    }))
                    .text_size(px(SFTP_TEXT_XS))
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.bg_hover)))
                    .child("Aa")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, _window, cx| {
                            this.toggle_sftp_preview_find_case(cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .w(px(SFTP_FIND_MATCH_WIDTH))
                    .text_align(gpui::TextAlign::Center)
                    .text_size(px(SFTP_TEXT_XS))
                    .text_color(rgb(theme.text_muted))
                    .child(match_label),
            )
            .child(
                div()
                    .size(px(SFTP_FIND_BUTTON_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.tokens.radii.sm))
                    .opacity(if can_step { 1.0 } else { 0.35 })
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.bg_hover)))
                    .child(Self::render_lucide_icon(
                        LucideIcon::ArrowUp,
                        SFTP_FIND_ICON_SIZE,
                        rgb(theme.text_muted),
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, _window, cx| {
                            this.sftp_preview_find_select_next(true, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .size(px(SFTP_FIND_BUTTON_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.tokens.radii.sm))
                    .opacity(if can_step { 1.0 } else { 0.35 })
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.bg_hover)))
                    .child(Self::render_lucide_icon(
                        LucideIcon::ArrowDown,
                        SFTP_FIND_ICON_SIZE,
                        rgb(theme.text_muted),
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, _window, cx| {
                            this.sftp_preview_find_select_next(false, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .size(px(SFTP_FIND_BUTTON_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.tokens.radii.sm))
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.bg_hover)))
                    .child(Self::render_lucide_icon(
                        LucideIcon::X,
                        SFTP_FIND_ICON_SIZE,
                        rgb(theme.text_muted),
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, window, cx| {
                            this.close_sftp_preview_find(window, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_sftp_editor_status(
        &self,
        status: Option<(String, gpui::Rgba)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (save_error, network_error, retry_count) = {
            let sftp_view = self.sftp_view.read(cx);
            (
                sftp_view.preview_editor_save_error.clone(),
                sftp_view.preview_editor_network_error,
                sftp_view.preview_editor_retry_count,
            )
        };
        if let Some(message) = save_error {
            if network_error {
                let label = if retry_count > 0 {
                    format!("{} ({retry_count})", self.i18n.t("sftp.preview.retry"))
                } else {
                    self.i18n.t("sftp.preview.retry")
                };
                return div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_color(rgb(SFTP_ORANGE))
                            .child(Self::render_lucide_icon(
                                LucideIcon::WifiOff,
                                SFTP_ICON_MD,
                                rgb(SFTP_ORANGE),
                            ))
                            .child(div().max_w(px(320.0)).truncate().child(
                                self.render_selectable_text_scoped(
                                    "sftp-editor-save-error",
                                    (),
                                    message,
                                    SFTP_ORANGE,
                                    cx,
                                ),
                            )),
                    )
                    .child(
                        div()
                            .h(px(20.0))
                            .px(px(8.0))
                            .rounded(px(self.tokens.radii.sm))
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(px(SFTP_TEXT_XS))
                            .text_color(rgb(SFTP_ORANGE))
                            .hover(|style| {
                                style.bg(rgba((SFTP_ORANGE << 8) | SFTP_EDITOR_RETRY_HOVER_ALPHA))
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _event, _window, cx| {
                                    this.retry_sftp_preview_editor_save(cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(Self::render_lucide_icon(
                                LucideIcon::RefreshCcw,
                                SFTP_ICON_SM,
                                rgb(SFTP_ORANGE),
                            ))
                            .child(label),
                    )
                    .into_any_element();
            }
            return div()
                .max_w(px(360.0))
                .truncate()
                .text_color(rgb(SFTP_RED))
                .child(message)
                .into_any_element();
        }

        if let Some((message, color)) = status {
            div()
                .max_w(px(360.0))
                .truncate()
                .text_color(color)
                .child(message)
                .into_any_element()
        } else {
            div().into_any_element()
        }
    }

    pub(in crate::workspace::sftp) fn render_sftp_preview_text(&self, text: String) -> AnyElement {
        div()
            .font_family(settings_mono_font_family(self.settings_store.settings()))
            .text_size(px(SFTP_TEXT_XS))
            .child(text)
            .into_any_element()
    }
}

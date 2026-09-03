use super::*;

pub(in crate::workspace) const HELP_WEBSITE_URL: &str = "https://oxideterm.app";
pub(in crate::workspace) const HELP_DOCUMENTATION_URL: &str = "https://oxideterm.app/docs";
pub(in crate::workspace) const HELP_GITHUB_URL: &str =
    "https://github.com/AnalyseDeCircuit/oxideterm";
pub(in crate::workspace) const HELP_ISSUES_URL: &str =
    "https://github.com/AnalyseDeCircuit/oxideterm/issues";
// Keep the in-app legal link aligned with the repository-level multilingual notice.
pub(in crate::workspace) const HELP_LEGAL_URL: &str =
    "https://github.com/AnalyseDeCircuit/oxideterm/blob/main/LEGAL.md";
pub(in crate::workspace) const HELP_LEGAL_MARKDOWN: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../LEGAL.md"));

// Product and library names stay untranslated so every locale uses their canonical spelling.
pub(in crate::workspace) const HELP_TECH_BADGES: [(&str, u32); 9] = [
    ("Rust", 0xf97316),
    ("GPUI", 0x38bdf8),
    ("Tokio", 0x3b82f6),
    ("alacritty_terminal", 0xf46d01),
    ("russh", 0xeab308),
    ("tree-sitter", 0x14b8a6),
    ("Wasmtime / WASI", 0xa855f7),
    ("redb", 0x22c55e),
    ("IronRDP", 0x6366f1),
];

pub(in crate::workspace) const HELP_LEGAL_NOTICE_WIDTH: f32 = 760.0;
pub(in crate::workspace) const HELP_LEGAL_NOTICE_HEIGHT: f32 = 720.0;

impl WorkspaceApp {
    pub(in crate::workspace) fn settings_help_section(
        &mut self,
        section_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match section_index {
            0 => self.help_version_card(cx),
            1 => self.help_diagnostics_card(cx),
            2 => self.help_tech_stack_card(),
            3 => self.help_resources_card(cx),
            4 => self.help_safety_card(),
            5 => self.help_legal_card(cx),
            _ => div().into_any_element(),
        }
    }

    pub(in crate::workspace) fn help_version_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let version_rows = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(self.help_key_value_row(
                "settings_view.help.app_name",
                "OxideTerm".to_string(),
                false,
                cx,
            ))
            .child(self.help_key_value_row(
                "settings_view.help.version",
                env!("CARGO_PKG_VERSION").to_string(),
                true,
                cx,
            ));

        let card = div()
            .w_full()
            .min_w(px(0.0))
            .rounded(px(self.tokens.radii.lg))
            .border_1()
            .border_color(rgb(self.tokens.ui.border))
            .p(px(self.tokens.metrics.settings_card_padding))
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(16.0))
                    .text_size(px(self.tokens.metrics.ui_text_sm))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(self.tokens.ui.text))
                    .child(
                        self.i18n
                            .t("settings_view.help.version_info")
                            .to_uppercase(),
                    ),
            )
            .child(version_rows);

        self.settings_card_surface(card, self.tokens.ui.bg_card)
            .into_any_element()
    }

    pub(in crate::workspace) fn help_diagnostics_card(&self, cx: &mut Context<Self>) -> AnyElement {
        // MemoryDiagnosticsPanel and the keyboard-shortcut reference are Tauri-only Help blocks.
        // GPUI keeps diagnostics lightweight: file logging is always available, while verbose
        // debug output is opt-in so normal sessions do not generate oversized logs.
        self.plain_settings_card(vec![
            self.card_title("settings_view.help.diagnostics"),
            self.bool_row(
                "settings_view.help.debug_logs",
                "settings_view.help.debug_logs_hint",
                self.settings_store.settings().diagnostics.debug_logging,
                set_diagnostics_debug_logging,
                cx,
            ),
            self.card_separator(),
            self.help_action_row(
                "settings_view.help.open_logs",
                "settings_view.help.open_logs_hint",
                self.i18n.t("settings_view.help.open"),
                LucideIcon::FolderOpen,
                |this, _event, _window, cx| this.open_help_log_directory(cx),
                cx,
            ),
        ])
    }

    pub(in crate::workspace) fn help_tech_stack_card(&self) -> AnyElement {
        let mut badges = div().flex().flex_row().flex_wrap().gap(px(8.0));
        for (label, color) in HELP_TECH_BADGES {
            badges = badges.child(
                div()
                    .rounded_full()
                    .bg(rgba((color << 8) | 0x26))
                    .px(px(10.0))
                    .py(px(4.0))
                    .text_size(px(self.tokens.metrics.ui_text_xs))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(color))
                    .child(label),
            );
        }

        self.plain_settings_card(vec![
            self.card_title("settings_view.help.tech_stack"),
            badges.into_any_element(),
        ])
    }

    pub(in crate::workspace) fn help_resources_card(&self, cx: &mut Context<Self>) -> AnyElement {
        self.plain_settings_card(vec![
            self.card_title("settings_view.help.resources"),
            self.help_resource_link(
                "settings_view.help.website",
                HELP_WEBSITE_URL,
                LucideIcon::ExternalLink,
                cx,
            ),
            self.help_resource_link(
                "settings_view.help.documentation",
                HELP_DOCUMENTATION_URL,
                LucideIcon::BookOpen,
                cx,
            ),
            self.help_resource_link(
                "settings_view.help.github",
                HELP_GITHUB_URL,
                LucideIcon::GitFork,
                cx,
            ),
            self.help_resource_link(
                "settings_view.help.issues",
                HELP_ISSUES_URL,
                LucideIcon::HelpCircle,
                cx,
            ),
            self.help_resource_link(
                "settings_view.help.disclaimer",
                HELP_LEGAL_URL,
                LucideIcon::Shield,
                cx,
            ),
        ])
    }

    pub(in crate::workspace) fn help_safety_card(&self) -> AnyElement {
        // Keep product guardrails visible without turning them into a blocking legal agreement.
        let safety_items = [
            "settings_view.help.safety_authorized",
            "settings_view.help.safety_connections",
            "settings_view.help.safety_prohibited",
            "settings_view.help.safety_privacy",
            "settings_view.help.safety_secrets",
            "settings_view.help.safety_ai",
        ];
        let mut safety_rows = div().flex().flex_col().gap(px(10.0));
        for key in safety_items {
            safety_rows = safety_rows.child(self.help_safety_row(key));
        }

        self.plain_settings_card(vec![
            self.card_title("settings_view.help.safety_title"),
            safety_rows.into_any_element(),
        ])
    }

    pub(in crate::workspace) fn help_safety_row(&self, key: &str) -> AnyElement {
        div()
            .flex()
            .items_start()
            .gap(px(10.0))
            .child(
                div()
                    .mt(px(7.0))
                    .size(px(5.0))
                    .rounded_full()
                    .bg(rgb(self.tokens.ui.accent)),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .text_size(px(self.tokens.metrics.ui_text_sm))
                    .line_height(px(20.0))
                    .text_color(rgb(self.tokens.ui.text_muted))
                    .child(self.i18n.t(key)),
            )
            .into_any_element()
    }

    pub(in crate::workspace) fn help_legal_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let copyright = self.i18n_with(
            "settings_view.help.copyright",
            &[
                ("year", chrono::Local::now().format("%Y").to_string()),
                ("author", "AnalyseDeCircuit".to_string()),
            ],
        );

        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .text_size(px(self.tokens.metrics.ui_text_xs))
            .text_color(rgb(self.tokens.ui.text_muted))
            .child(self.render_selectable_text_scoped(
                "settings-help-legal",
                "copyright",
                copyright,
                self.tokens.ui.text_muted,
                cx,
            ))
            .child(self.render_selectable_text_scoped(
                "settings-help-legal",
                "license",
                self.i18n.t("settings_view.help.license"),
                self.tokens.ui.text_muted,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::workspace) fn help_key_value_row(
        &self,
        label_key: &str,
        value: String,
        mono: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut value_el = div()
            .text_size(px(self.tokens.metrics.ui_text_sm))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(rgb(self.tokens.ui.text))
            .child(self.render_selectable_text_scoped(
                "settings-help-value",
                label_key,
                value,
                self.tokens.ui.text,
                cx,
            ));
        if mono {
            value_el =
                value_el.font_family(settings_mono_font_family(self.settings_store.settings()));
        }

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                div()
                    .text_size(px(self.tokens.metrics.ui_text_sm))
                    .text_color(rgb(self.tokens.ui.text_muted))
                    .child(self.i18n.t(label_key)),
            )
            .child(value_el)
            .into_any_element()
    }

    pub(in crate::workspace) fn help_action_row(
        &self,
        label_key: &str,
        hint_key: &str,
        button_label: String,
        icon: LucideIcon,
        listener: impl Fn(&mut Self, &MouseDownEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.setting_row(
            label_key,
            hint_key,
            self.help_outline_button(button_label, icon, listener, cx),
            cx,
        )
    }

    pub(in crate::workspace) fn help_outline_button(
        &self,
        label: String,
        icon: LucideIcon,
        listener: impl Fn(&mut Self, &MouseDownEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.workspace_toolbar_action_button(
            label,
            Some(Self::render_lucide_icon(icon, 14.0, rgb(self.tokens.ui.text))),
            ToolbarButtonOptions {
                button: ButtonOptions {
                    variant: ButtonVariant::Outline,
                    size: ButtonSize::Sm,
                    radius: ButtonRadius::Md,
                    disabled: false,
                },
                icon_position: ToolbarButtonIconPosition::Leading,
                ..ToolbarButtonOptions::default()
            },
            cx.listener(move |this, event, window, cx| {
                listener(this, event, window, cx);
                cx.stop_propagation();
            }),
        )
        .into_any_element()
    }

    pub(in crate::workspace) fn help_resource_link(
        &self,
        label_key: &str,
        url: &'static str,
        icon: LucideIcon,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .rounded(px(self.tokens.radii.md))
            .px(px(12.0))
            .py(px(10.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .cursor_pointer()
            .hover(|row| row.bg(rgb(self.tokens.ui.bg_hover)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, _window, cx| {
                    if url == HELP_LEGAL_URL {
                        this.open_help_legal_notice(cx);
                    } else {
                        this.open_help_url(url, cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(Self::render_lucide_icon(
                        icon,
                        18.0,
                        rgb(self.tokens.ui.text_muted),
                    ))
                    .child(
                        div()
                            .text_size(px(self.tokens.metrics.ui_text_sm))
                            .text_color(rgb(self.tokens.ui.text))
                            .child(self.i18n.t(label_key)),
                    ),
            )
            .child(Self::render_lucide_icon(
                LucideIcon::ExternalLink,
                14.0,
                rgb(self.tokens.ui.text_muted),
            ))
            .into_any_element()
    }

    pub(in crate::workspace) fn open_help_legal_notice(&mut self, cx: &mut Context<Self>) {
        self.settings_legal_notice_scroll = MarkdownVirtualListScrollHandle::new();
        self.overlay.update(cx, |overlay, cx| {
            overlay.open_confirm(WorkspaceOverlayConfirmKind::LegalNotice, cx);
        });
    }

    pub(in crate::workspace) fn close_help_legal_notice(&mut self, cx: &mut Context<Self>) {
        let delay = oxideterm_gpui_ui::motion::duration(
            &self.tokens,
            oxideterm_gpui_ui::motion::MotionDuration::Overlay,
        );
        self.overlay.update(cx, |overlay, cx| {
            overlay.begin_confirm_exit(false, delay, cx);
        });
    }

    pub(in crate::workspace) fn handle_help_legal_notice_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(snapshot) = self.overlay.read(cx).confirm_snapshot() else {
            return false;
        };
        if !matches!(snapshot.kind, WorkspaceOverlayConfirmKind::LegalNotice) {
            return false;
        }
        if snapshot.phase == oxideterm_gpui_ui::motion::ExitPhase::Exiting {
            return true;
        }
        let key_action = self.overlay.update(cx, |overlay, cx| {
            overlay.handle_confirm_key(
                event.keystroke.key.as_str(),
                event.keystroke.modifiers.shift,
                event.keystroke.modifiers.platform || event.keystroke.modifiers.control,
                cx,
            )
        });
        match key_action {
            Some(
                WorkspaceOverlayConfirmKeyAction::Cancel
                | WorkspaceOverlayConfirmKeyAction::Confirm,
            ) => {
                self.close_help_legal_notice(cx);
                true
            }
            Some(WorkspaceOverlayConfirmKeyAction::Handled) => true,
            None => false,
        }
    }

    pub(in crate::workspace) fn render_help_legal_notice_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(snapshot) = self.overlay.read(cx).confirm_snapshot() else {
            return div().into_any_element();
        };
        if !matches!(snapshot.kind, WorkspaceOverlayConfirmKind::LegalNotice) {
            return div().into_any_element();
        }
        let mut options = self.localized_markdown_options();
        options.base_font_size = self.tokens.metrics.ui_text_sm;
        options.block_gap = 8.0;
        let code_actions = self.markdown_mermaid_actions(cx);

        let backdrop = dismissible_dialog_backdrop().on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event, _window, cx| {
                this.close_help_legal_notice(cx);
                cx.stop_propagation();
            }),
        );
        let form = overlay_content_boundary(
            dialog_content(&self.tokens)
                .flex()
                .flex_col()
                .w(px(HELP_LEGAL_NOTICE_WIDTH))
                .max_w(relative(0.92))
                .h(px(HELP_LEGAL_NOTICE_HEIGHT))
                .max_h(relative(0.90))
                .child(
                    dialog_header(&self.tokens)
                        .child(dialog_title(
                            &self.tokens,
                            self.i18n.t("settings_view.help.disclaimer"),
                        ))
                        .child(dialog_description(
                            &self.tokens,
                            self.i18n.t("settings_view.help.legal_notice_description"),
                        )),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.0))
                        .p(px(16.0))
                        .bg(rgb(self.tokens.ui.bg))
                        .text_color(rgb(self.tokens.ui.text))
                        .child(markdown_virtual_with_code_actions(
                            "settings-help-legal-notice-markdown",
                            &self.tokens,
                            HELP_LEGAL_MARKDOWN,
                            &options,
                            &self.settings_legal_notice_scroll,
                            &code_actions,
                        )),
                )
                .child(
                    dialog_footer(&self.tokens).child(self.standard_footer_action_button(
                        self.i18n.t("settings_view.help.legal_notice_close"),
                        ButtonVariant::Secondary,
                        ConfirmDialogAction::Cancel,
                        false,
                        |this, _event, _window, cx| {
                            this.close_help_legal_notice(cx);
                        },
                        cx,
                    )),
                ),
        );
        settings_dialog_transition(
            &self.tokens,
            "help-legal-notice-form",
            backdrop,
            form,
            snapshot.phase,
        )
    }

    pub(in crate::workspace) fn open_help_url(
        &mut self,
        url: &'static str,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = open_external_url(url) {
            self.push_ai_settings_toast(error.to_string(), TerminalNoticeVariant::Error, cx);
            cx.notify();
        }
    }

    pub(in crate::workspace) fn open_help_log_directory(&mut self, cx: &mut Context<Self>) {
        let log_dir = self.help_log_directory();
        let opened = std::fs::create_dir_all(&log_dir)
            .and_then(|()| open_path_external(&log_dir))
            .map_err(|error| error.to_string());
        if let Err(error) = opened {
            self.push_ai_settings_toast(error, TerminalNoticeVariant::Error, cx);
            cx.notify();
        }
    }

    pub(in crate::workspace) fn help_log_directory(&self) -> std::path::PathBuf {
        // Tauri stores logs under the app data directory. Native settings use
        // the same data root, so derive logs beside settings.json.
        self.settings_store
            .path()
            .parent()
            .map(|parent| parent.join("logs"))
            .unwrap_or_else(|| std::path::PathBuf::from("logs"))
    }

    pub(in crate::workspace) fn language_label(&self, language: Language) -> String {
        match language {
            Language::De => "Deutsch",
            Language::En => "English",
            Language::EsEs => "Español (España)",
            Language::FrFr => "Français (France)",
            Language::It => "Italiano",
            Language::Ko => "한국어",
            Language::PtBr => "Português (Brasil)",
            Language::Vi => "Tiếng Việt",
            Language::Ja => "日本語",
            Language::ZhCn => "简体中文",
            Language::ZhTw => "繁體中文",
        }
        .to_string()
    }
}

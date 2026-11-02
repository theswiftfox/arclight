//! The main (play) screen: top bar, client tabs, hero area and play panel.
use std::path::PathBuf;

use iced::{
    Alignment, Background, Border, Color, Element, Length, mouse,
    widget::{
        button, column, container, image, mouse_area, row, scrollable, space, stack, text, tooltip,
    },
};

use super::{
    App, EMOJI_FONT, Message, NewsState, SettingsSection, SettingsTarget, effective_runner_label,
    theme,
};
use crate::{
    config::GameClient,
    launcher::{
        bnet::AuthStatus,
        news::{self, NewsItem},
    },
};

impl App {
    pub(super) fn view_main(&self) -> Element<'_, Message> {
        let selected = self.config.selected_client();
        column![
            self.view_top_bar(),
            self.view_client_tabs(selected),
            self.view_hero(selected),
            self.view_play_panel(selected),
        ]
        .into()
    }

    fn view_top_bar(&self) -> Element<'_, Message> {
        let brand = text("ARCLIGHT").size(20).color(theme::COLOR_GOLD);
        let nav = row![
            nav_button("Play", true, None),
            nav_button("Settings", false, Some(Message::GoToSettings)),
            nav_button("Logs", false, Some(Message::OpenLogs)),
        ]
        .spacing(4);

        container(
            row![
                brand,
                nav,
                space().width(Length::Fill),
                self.view_bnet_chip()
            ]
            .spacing(24)
            .align_y(Alignment::Center),
        )
        .padding([10, 24])
        .width(Length::Fill)
        .style(panel_style)
        .into()
    }

    fn view_bnet_chip(&self) -> Element<'_, Message> {
        let (color, status) = match &self.auth_status {
            AuthStatus::NotInstalled => {
                (theme::COLOR_DANGER, "Battle.net not installed".to_owned())
            }
            AuthStatus::NotLoggedIn => (theme::COLOR_DANGER, "Not logged in".to_owned()),
            AuthStatus::BnetLoggedIn => (theme::COLOR_SUCCESS, "Battle.net logged in".to_owned()),
            AuthStatus::WowReady { game_account } => (
                theme::COLOR_SUCCESS,
                format!("Battle.net logged in ({game_account})"),
            ),
        };
        let dot = container(space())
            .width(10)
            .height(10)
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(Background::Color(color)),
                border: Border::default().rounded(5),
                ..Default::default()
            });
        let badge: Element<'_, Message> = match &self.bnet_icon {
            Some(icon) => image(image::Handle::from_path(icon))
                .width(20)
                .height(20)
                .into(),
            None => container(text("B").size(12).color(Color::WHITE))
                .center_x(20)
                .center_y(20)
                .style(|_theme: &iced::Theme| container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.0, 0.45, 0.85))),
                    border: Border::default().rounded(5),
                    ..Default::default()
                })
                .into(),
        };
        let chip = |label: &'static str, message: Option<Message>| {
            button(text(label).size(13))
                .padding([6, 12])
                .style(theme::surface_button)
                .on_press_maybe(message)
        };

        let mut controls = row![].spacing(8).align_y(Alignment::Center);
        if self.installing {
            controls = controls.push(chip("Cancel installation", Some(Message::CancelInstall)));
        } else if self.bnet_busy {
            controls = controls.push(chip("Battle.net running...", None));
            controls = controls.push(chip(
                "Force Quit",
                (self.bnet_started && !self.killing_bnet).then_some(Message::KillBnet),
            ));
        } else if self.auth_status == AuthStatus::NotInstalled {
            controls = controls.push(chip("Install Battle.net", Some(Message::InstallBnet)));
        } else {
            controls = controls.push(tooltip(
                button(
                    row![badge, text("Battle.net").size(13)]
                        .spacing(8)
                        .align_y(Alignment::Center),
                )
                .padding([5, 12])
                .style(theme::surface_button)
                .on_press(Message::LaunchBnet),
                container(text(status).size(12))
                    .padding(6)
                    .style(container::rounded_box),
                tooltip::Position::Bottom,
            ));
        }
        let refresh = button(text("\u{1F504}").font(EMOJI_FONT).size(13))
            .padding([6, 10])
            .style(theme::surface_button)
            .on_press_maybe((!self.auth_busy).then_some(Message::RefreshAuthStatus));

        row![dot, controls, refresh]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
    }

    fn view_client_tabs(&self, selected: Option<&GameClient>) -> Element<'_, Message> {
        let mut tabs = row![].spacing(6).align_y(Alignment::Center);
        for client in &self.config.game_clients {
            let is_selected = selected.is_some_and(|selected| selected.id == client.id);
            let available = self.available_clients.contains(&client.id);
            let outdated = self
                .client_versions
                .get(&client.id)
                .is_some_and(|version| version.outdated);

            let mut label = row![text(&client.name).size(14)]
                .spacing(6)
                .align_y(Alignment::Center);
            if outdated {
                label = label.push(text("!").size(14).color(theme::COLOR_WARNING));
            }
            let hovered = self.hovered_tab.as_ref() == Some(&client.id);
            let tab = container(label)
                .padding([7, 16])
                .style(move |theme: &iced::Theme| {
                    tab_style(theme, hovered, is_selected, available)
                });
            tabs = tabs.push(
                mouse_area(tab)
                    .on_press(Message::ClientTabPressed(client.id.clone()))
                    .on_enter(Message::ClientTabEntered(client.id.clone()))
                    .on_exit(Message::ClientTabExited(client.id.clone()))
                    .interaction(if self.tab_drag.is_some() {
                        mouse::Interaction::Grabbing
                    } else {
                        mouse::Interaction::Pointer
                    }),
            );
        }

        let scroll = scrollable(tabs)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(4).scroller_width(4),
            ))
            .width(Length::Fill);
        let add = button(text("+").size(16))
            .padding([4, 12])
            .style(theme::surface_button)
            .on_press(Message::OpenSettings(SettingsTarget::Section(
                SettingsSection::Clients,
            )));

        container(row![scroll, add].spacing(14).align_y(Alignment::Center))
            .padding([8, 24])
            .width(Length::Fill)
            .style(panel_style)
            .into()
    }

    fn view_hero<'a>(&'a self, selected: Option<&'a GameClient>) -> Element<'a, Message> {
        let tint = selected.map_or(theme::COLOR_PANEL, |client| theme::tint_for(&client.id));
        let name = selected.map_or("No client selected", |client| client.name.as_str());
        let hint = match selected {
            None => "Add a game client in Settings",
            Some(client) if !self.available_clients.contains(&client.id) => {
                "Game executable not found. Check the install path in Settings"
            }
            Some(_) => "World of Warcraft",
        };

        let background = selected.and_then(|client| {
            self.config.background_for(client).or_else(|| {
                self.config
                    .official_artwork
                    .then(|| self.official_art.get(&client.id).map(PathBuf::as_path))
                    .flatten()
            })
        });
        let has_image = background.is_some();

        let content = container(
            column![
                space().height(Length::Fill),
                text(name).size(56).color(Color::WHITE),
                text(hint).size(16).color(theme::COLOR_HINT),
            ]
            .spacing(4),
        )
        .padding([32, 40])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &iced::Theme| {
            // End on the panel color so there is no step where the hero meets the panel.
            let base = theme::COLOR_PANEL;
            let angle = iced::Radians(std::f32::consts::PI);
            let gradient = if has_image {
                // Keep the picture visible, and fade into the panel below it.
                iced::gradient::Linear::new(angle)
                    .add_stop(
                        0.0,
                        Color {
                            a: 0.30,
                            ..Color::BLACK
                        },
                    )
                    .add_stop(
                        0.5,
                        Color {
                            a: 0.0,
                            ..Color::BLACK
                        },
                    )
                    .add_stop(1.0, base)
            } else {
                iced::gradient::Linear::new(angle)
                    .add_stop(0.0, tint)
                    .add_stop(1.0, base)
            };
            container::Style {
                background: Some(Background::Gradient(iced::Gradient::Linear(gradient))),
                ..Default::default()
            }
        });

        let hero: Element<'a, Message> = match background {
            // The hero's height is fractional. Keep the picture off its last pixel row so
            // the anti-aliased overlay never lets a bright sliver of it show above the panel.
            Some(path) => stack![
                container(
                    image(image::Handle::from_path(path))
                        .content_fit(iced::ContentFit::Cover)
                        .width(Length::Fill)
                        .height(Length::Fill)
                )
                .padding(iced::Padding {
                    bottom: 2.0,
                    ..iced::Padding::ZERO
                }),
                content
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            None => content.into(),
        };

        stack![hero, self.view_news_layer()]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The "Show news" button, or the news panel over the right two thirds of the hero.
    fn view_news_layer(&self) -> Element<'_, Message> {
        if !self.news_open {
            return container(
                button(text("Show news").size(13))
                    .padding([6, 14])
                    .style(news_button_style)
                    .on_press(Message::ToggleNews),
            )
            .padding(16)
            .align_right(Length::Fill)
            .into();
        }

        let header = row![
            text("News").size(18).color(theme::COLOR_GOLD),
            space().width(Length::Fill),
            button(text("Patch notes").size(13))
                .padding([5, 12])
                .style(theme::surface_button)
                .on_press(Message::OpenUrl(news::patch_notes_url(
                    self.config.news_locale
                ))),
            button(text("Hide news").size(13))
                .padding([5, 12])
                .style(theme::surface_button)
                .on_press(Message::ToggleNews),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let body: Element<'_, Message> = match &self.news {
            NewsState::Loaded(items, _) => {
                let now = chrono::Utc::now();
                let cards = items.iter().map(|item| self.view_news_card(item, now));
                scrollable(
                    column(cards)
                        .spacing(8)
                        .padding(iced::Padding::ZERO.right(14)),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            NewsState::Failed(error) => column![
                text("The news could not be loaded.").size(14),
                text(error).size(12).color(theme::COLOR_MUTED),
                button(text("Retry").size(13))
                    .padding([5, 12])
                    .style(theme::surface_button)
                    .on_press(Message::RetryNews),
            ]
            .spacing(8)
            .into(),
            NewsState::Idle | NewsState::Loading => text("Loading news...")
                .size(14)
                .color(theme::COLOR_MUTED)
                .into(),
        };

        let panel = container(column![header, body].spacing(12))
            .padding(16)
            .width(Length::FillPortion(2))
            .height(Length::Fill)
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.92,
                    ..theme::COLOR_PANEL
                })),
                border: Border {
                    color: theme::COLOR_BORDER,
                    width: 1.0,
                    radius: 14.into(),
                },
                ..container::Style::default()
            });

        container(row![space().width(Length::FillPortion(1)), panel])
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_news_card<'a>(
        &'a self,
        item: &'a NewsItem,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Element<'a, Message> {
        let mut text_column = column![text(&item.title).size(15).color(Color::WHITE)].spacing(4);
        if !item.description.is_empty() {
            text_column =
                text_column.push(text(&item.description).size(12).color(theme::COLOR_HINT));
        }
        text_column =
            text_column.push(text(item.age_label(now)).size(11).color(theme::COLOR_MUTED));

        let mut card = row![].spacing(12);
        if self.config.news_thumbnails
            && let Some(path) = &item.thumbnail
        {
            card = card.push(
                container(
                    image(image::Handle::from_path(path))
                        .content_fit(iced::ContentFit::Cover)
                        .width(120)
                        .height(68),
                )
                .clip(true)
                .style(|_theme: &iced::Theme| container::Style {
                    border: Border::default().rounded(8),
                    ..container::Style::default()
                }),
            );
        }
        card = card.push(text_column.width(Length::Fill));

        button(card)
            .padding(10)
            .width(Length::Fill)
            .style(news_card_style)
            .on_press(Message::OpenUrl(item.url.clone()))
            .into()
    }

    fn view_play_panel(&self, selected: Option<&GameClient>) -> Element<'_, Message> {
        let can_play = selected.is_some_and(|client| self.available_clients.contains(&client.id))
            && !self.launching
            && !self.wow_running
            && !self.killing_wow
            && !self.saving
            && !self.installing;

        let runner_info = text(effective_runner_label(
            selected,
            self.selected_runner.as_ref(),
            self.automatic_runner.as_ref(),
            &self.detected_runners,
        ))
        .size(12)
        .color(theme::COLOR_MUTED);
        let game_path_info = match (&self.config.game_path, selected) {
            (Some(path), Some(client)) => {
                text(format!("Game: {}", path.join(&client.executable).display()))
            }
            _ => text("Game path not set. Configure it in Settings"),
        }
        .size(12)
        .color(theme::COLOR_MUTED);
        let info = column![
            self.view_version_info(selected),
            runner_info,
            game_path_info,
            text(&self.status).size(12),
        ]
        .spacing(4)
        .align_x(Alignment::End);

        // The gear opens the settings card of the selected client.
        let client_settings = selected
            .and_then(|client| {
                self.config
                    .game_clients
                    .iter()
                    .position(|candidate| candidate.id == client.id)
            })
            .map_or(
                SettingsTarget::Section(SettingsSection::Clients),
                SettingsTarget::Client,
            );
        let controls: Element<'_, Message> = if self.wow_running {
            row![
                button(
                    text(format!(
                        "{} is running",
                        self.wow_client
                            .as_ref()
                            .map_or("WoW", |client| &client.name)
                    ))
                    .size(16)
                )
                .padding([14, 32]),
                button("Force Quit")
                    .padding([14, 24])
                    .on_press_maybe((!self.killing_wow).then_some(Message::KillWow)),
            ]
            .spacing(8)
            .into()
        } else {
            row![
                button(text("PLAY").size(22))
                    .padding([12, 64])
                    .style(play_style)
                    .on_press_maybe(can_play.then_some(Message::Launch)),
                button(text("\u{2699}").font(EMOJI_FONT).size(22))
                    .padding([12, 16])
                    .style(play_style)
                    .on_press(Message::OpenSettings(client_settings)),
            ]
            .spacing(2)
            .into()
        };

        container(
            row![controls, space().width(Length::Fill), info]
                .spacing(24)
                .align_y(Alignment::Center),
        )
        .padding([18, 24])
        .width(Length::Fill)
        .style(panel_style)
        .into()
    }

    fn view_version_info(&self, client: Option<&GameClient>) -> Element<'_, Message> {
        let Some(info) = client.and_then(|client| self.client_versions.get(&client.id)) else {
            return space().height(0).into();
        };
        let mut line = row![text(format!("installed {}", info.installed)).size(12)]
            .spacing(8)
            .align_y(Alignment::Center);
        if let Some(latest) = &info.latest {
            line = line.push(text(format!("latest {latest}")).size(12));
            if info.outdated {
                let badge = container(text("!").size(12).color(Color::BLACK))
                    .center_x(18)
                    .center_y(18)
                    .style(|_theme: &iced::Theme| container::Style {
                        background: Some(Background::Color(theme::COLOR_WARNING)),
                        border: Border::default().rounded(9),
                        ..Default::default()
                    });
                line = line.push(tooltip(
                    badge,
                    container(
                        text(format!("Update available: {} → {latest}", info.installed)).size(12),
                    )
                    .padding(6)
                    .style(container::rounded_box),
                    tooltip::Position::Top,
                ));
            }
        }
        line.into()
    }
}

fn news_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(Color {
            a: if hovered { 0.95 } else { 0.8 },
            ..theme::COLOR_PANEL
        })),
        text_color: theme::COLOR_GOLD,
        border: Border {
            color: theme::COLOR_GOLD,
            width: 1.0,
            radius: 14.into(),
        },
        ..button::Style::default()
    }
}

fn news_card_style(theme: &iced::Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered {
            theme::COLOR_SURFACE_HOVER
        } else {
            theme::COLOR_SURFACE
        })),
        text_color: theme.palette().text,
        border: Border::default().rounded(10),
        ..button::Style::default()
    }
}

fn panel_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::COLOR_PANEL)),
        ..Default::default()
    }
}

fn nav_button(
    label: &'static str,
    active: bool,
    on_press: Option<Message>,
) -> Element<'static, Message> {
    button(text(label).size(14))
        .padding([6, 14])
        .style(move |theme: &iced::Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let tint = |alpha: f32| Color {
                a: alpha,
                ..theme::COLOR_GOLD
            };
            button::Style {
                background: if active {
                    Some(Background::Color(tint(0.16)))
                } else if hovered {
                    Some(Background::Color(Color {
                        a: 0.08,
                        ..Color::WHITE
                    }))
                } else {
                    None
                },
                text_color: if active {
                    theme::COLOR_GOLD
                } else {
                    theme.palette().text
                },
                border: Border::default().rounded(4),
                ..button::Style::default()
            }
        })
        .on_press_maybe(on_press)
        .into()
}

fn tab_style(
    theme: &iced::Theme,
    hovered: bool,
    selected: bool,
    available: bool,
) -> container::Style {
    let text_color = if selected {
        theme::COLOR_GOLD
    } else if available {
        theme.palette().text
    } else {
        theme::COLOR_MUTED
    };
    container::Style {
        background: if selected {
            Some(Background::Color(Color {
                a: 0.16,
                ..theme::COLOR_GOLD
            }))
        } else if hovered {
            Some(Background::Color(Color {
                a: 0.08,
                ..Color::WHITE
            }))
        } else {
            None
        },
        text_color: Some(text_color),
        border: Border {
            color: if selected {
                theme::COLOR_GOLD
            } else {
                Color::TRANSPARENT
            },
            width: 1.0,
            radius: 4.into(),
        },
        ..container::Style::default()
    }
}

fn play_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Disabled => (
            Color {
                a: 0.35,
                ..theme::COLOR_GOLD
            },
            Color::from_rgba(0.0, 0.0, 0.0, 0.5),
        ),
        button::Status::Hovered | button::Status::Pressed => {
            (Color::from_rgb(0.96, 0.74, 0.25), Color::BLACK)
        }
        button::Status::Active => (theme::COLOR_GOLD, Color::BLACK),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border::default().rounded(3),
        ..button::Style::default()
    }
}

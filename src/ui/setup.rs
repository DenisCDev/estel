//! First-run questions reuse the settings persistence and bounded hardware helpers.

use eframe::egui::{self, Color32, Frame, Margin, RichText};

use super::{MINT, PAPER, SettingsApp, camera_calibration_label, poster_font, time_row};
use crate::config::ScreenWindowRelation;

const TITLES: [&str; 5] = [
    "Onde você usa o Estel?",
    "Como a luz da janela chega à tela?",
    "Quer adaptar o brilho à luz ambiente?",
    "Qual é a sua rotina e seu brilho confortável?",
    "Confira suas escolhas",
];

impl SettingsApp {
    pub(super) fn setup_panel(&mut self, ctx: &egui::Context) -> bool {
        let Some(step) = self.setup_step else {
            return false;
        };
        egui::CentralPanel::default()
            .frame(Frame::new().fill(PAPER).inner_margin(Margin::same(25)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("setup", step))
                    .show(ui, |ui| {
                        ui.label(RichText::new("ESTEL / CONFIGURAÇÃO INICIAL").font(poster_font(24.0)));
                        ui.label(format!("Etapa {} de {}", step + 1, TITLES.len()));
                        ui.heading(TITLES[step]);
                        ui.label("Você pode deixar opções para depois e revisitar este guia pelo painel. As escolhas ficam salvas neste computador.");
                        if !self.cfg.setup_completed {
                            ui.label("Os ajustes de tela e som começam quando você concluir o guia.");
                        }
                        ui.add_space(18.0);
                        match step {
                            0 => self.setup_location(ui),
                            1 => self.setup_window(ui),
                            2 => self.setup_camera(ui),
                            3 => self.setup_comfort(ui),
                            _ => self.setup_review(ui),
                        }
                        ui.add_space(20.0);
                        let pending = self.calibration_capture.is_some()
                            || self.location_scan.is_some()
                            || self.location_request
                            || self.place_search.is_some()
                            || self.light_sensor_scan.is_some();
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(step > 0 && !pending, egui::Button::new("VOLTAR")).clicked() {
                                self.setup_step = Some(step - 1);
                            }
                            let label = if step + 1 == TITLES.len() { "CONCLUIR CONFIGURAÇÃO" } else { "CONTINUAR" };
                            if ui.add_enabled(!pending, egui::Button::new(label).fill(MINT)).clicked() {
                                if step + 1 == TITLES.len() {
                                    self.cfg.setup_completed = true;
                                    self.touch();
                                    self.flush();
                                    if !self.dirty {
                                        self.setup_step = None;
                                        self.status = "Configuração concluída. Você pode ajustar tudo pelo painel.".into();
                                    }
                                } else {
                                    self.setup_step = Some(step + 1);
                                }
                            }
                        });
                        if pending {
                            ui.label("Aguarde a leitura terminar para continuar. Você pode cancelar a captura da câmera abaixo da leitura.");
                        }
                    });
            });
        true
    }

    fn setup_location(&mut self, ui: &mut egui::Ui) {
        ui.label("A cidade ajuda a calcular nascer e pôr do sol. A localização do Windows é opcional; também é possível escolher uma cidade ou informar coordenadas.");
        if ui
            .checkbox(&mut self.cfg.location_auto, "Usar localização do Windows")
            .changed()
        {
            self.location_request = self.cfg.location_auto;
            self.location_scan = None;
            self.location_success = false;
            self.location_error = None;
            self.touch();
        }
        if self.location_scan.is_some() || self.location_request {
            ui.spinner();
            ui.label("Consultando o Windows; você decide se permite o acesso. Prazo máximo de 42 segundos.");
        }
        if let Some(error) = &self.location_error {
            ui.colored_label(Color32::from_rgb(160, 40, 30), error);
            if ui.button("TENTAR LOCALIZAÇÃO NOVAMENTE").clicked() {
                self.location_request = true;
            }
            ui.label("Se preferir, desative a opção e use cidade ou coordenadas.");
        }
        if self.location_success {
            ui.label("Localização recebida do Windows.");
        }
        ui.label("Cidade ou bairro");
        if ui.text_edit_singleline(&mut self.place_query).changed() {
            self.place_results.clear();
            self.place_error = None;
            self.place_search = None;
        }
        let valid = (3..=100).contains(&self.place_query.trim().chars().count());
        if ui
            .add_enabled(
                valid && self.place_search.is_none(),
                egui::Button::new("BUSCAR CIDADE"),
            )
            .clicked()
        {
            self.start_place_search();
        }
        ui.label("Ao buscar, o texto é enviado ao Open-Meteo. Escolha um resultado para usar a cidade. Buscar uma cidade não ativa o clima; essa opção fica no painel.");
        if self.place_search.is_some() {
            ui.spinner();
            ui.label("Buscando cidades…");
        }
        if let Some(error) = &self.place_error {
            ui.colored_label(Color32::from_rgb(160, 40, 30), error);
        }
        let mut selected = None;
        for (index, place) in self.place_results.iter().enumerate() {
            if ui.button(place.label()).clicked() {
                selected = Some(index);
            }
        }
        if let Some(index) = selected {
            let place = &self.place_results[index];
            self.selected_place = Some(place.label());
            self.cfg.latitude = place.latitude;
            self.cfg.longitude = place.longitude;
            self.cfg.location_auto = false;
            self.location_scan = None;
            self.location_request = false;
            self.location_success = false;
            self.place_results.clear();
            self.touch();
        }
        if let Some(place) = &self.selected_place {
            ui.label(format!("Cidade selecionada: {place}"));
        }
        ui.horizontal(|ui| {
            ui.label("Latitude");
            let latitude = ui
                .add(
                    egui::DragValue::new(&mut self.cfg.latitude)
                        .speed(0.01)
                        .range(-90.0..=90.0),
                )
                .changed();
            ui.label("Longitude");
            let longitude = ui
                .add(
                    egui::DragValue::new(&mut self.cfg.longitude)
                        .speed(0.01)
                        .range(-180.0..=180.0),
                )
                .changed();
            if latitude || longitude {
                self.cfg.location_auto = false;
                self.location_request = false;
                self.location_scan = None;
                self.location_success = false;
                self.selected_place = None;
                self.touch();
            }
        });
        ui.label(format!("Coordenadas atuais: {:.4}, {:.4}. Em uma instalação nova, São Paulo é a referência inicial; você pode continuar com ela e ajustar depois.", self.cfg.latitude, self.cfg.longitude));
    }

    fn setup_window(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(
                &mut self.cfg.window_near,
                "Uma janela ilumina este ambiente perto da tela",
            )
            .changed()
        {
            self.touch();
        }
        if self.cfg.window_near {
            ui.label("Abra a bússola do celular e aponte o topo do aparelho do interior do cômodo para fora da janela. Leia a direção em graus, longe de ímãs e objetos metálicos. É uma referência aproximada, não a sua localização geográfica.");
            ui.label("Se a bússola oferecer a opção, use o norte verdadeiro (geográfico), a referência usada para estimar a posição do sol.");
            ui.label("Norte: 0° · Leste: 90° · Sul: 180° · Oeste: 270°. Se não souber, deixe a direção indefinida.");
            let mut known = self.cfg.window_azimuth_deg.is_some();
            if ui.checkbox(&mut known, "Sei a direção da janela").changed() {
                self.cfg.window_azimuth_deg = known.then_some(0.0);
                self.touch();
            }
            if let Some(direction) = &mut self.cfg.window_azimuth_deg {
                ui.label("Direção indicada pela bússola");
                if ui
                    .add(
                        egui::DragValue::new(direction)
                            .range(0.0..=359.0)
                            .suffix("°"),
                    )
                    .changed()
                {
                    self.touch();
                }
            }
            ui.label("Olhando para a tela, onde fica a janela?");
            for (relation, label) in [
                (
                    ScreenWindowRelation::Front,
                    "Atrás da tela, no meu campo de visão",
                ),
                (
                    ScreenWindowRelation::Back,
                    "Atrás de mim, podendo refletir na tela",
                ),
                (ScreenWindowRelation::Side, "Ao lado da tela"),
            ] {
                if ui
                    .radio_value(&mut self.cfg.screen_window_relation, relation, label)
                    .changed()
                {
                    self.touch();
                }
            }
        } else {
            ui.label(
                "Sem janela próxima, o Estel pode seguir o horário e a luz ambiente opcional.",
            );
        }
    }

    fn setup_camera(&mut self, ui: &mut egui::Ui) {
        ui.label("Opcional: use um sensor de luz do Windows ou a câmera para ajustar o brilho. A câmera processa quadros localmente, não grava nem envia imagens e não analisa pessoas. Ela exige duas referências; não mede lux.");
        if ui
            .checkbox(&mut self.cfg.ambient_enabled, "Adaptar à luz ambiente")
            .changed()
        {
            self.touch();
        }
        if !self.cfg.ambient_enabled {
            ui.label("Sem captura da câmera. O brilho seguirá o horário; você pode ativar e calibrar depois no painel.");
            return;
        }
        if ui
            .checkbox(
                &mut self.cfg.ambient_prefer_light_sensor,
                "Preferir sensor de luz do Windows, quando disponível",
            )
            .changed()
        {
            self.touch();
        }
        if ui
            .add_enabled(
                self.light_sensor_scan.is_none(),
                egui::Button::new("VERIFICAR SENSOR DE LUZ"),
            )
            .clicked()
        {
            self.probe_light_sensor();
        }
        if self.light_sensor_scan.is_some() {
            ui.spinner();
            ui.label("Verificando sensor… prazo máximo de 5 segundos.");
        }
        if let Some(message) = &self.light_sensor_status {
            ui.label(message);
        }
        ui.label("Câmera para usar quando não houver sensor");
        let cameras = self.camera_names.clone();
        for (index, camera) in cameras.iter().enumerate() {
            let selected = self.cfg.ambient_camera_id.as_ref() == Some(&camera.device_id);
            if ui
                .add_enabled(
                    self.calibration_capture.is_none(),
                    egui::Button::new(&camera.name).selected(selected),
                )
                .clicked()
                && !selected
            {
                self.cfg.ambient_camera_id = Some(camera.device_id.clone());
                self.cfg.ambient_camera_index = index;
                self.calibration_dark = None;
                self.calibration_message = None;
                self.touch();
            }
        }
        if self.camera_scan_pending {
            ui.spinner();
            ui.label("Buscando câmeras… prazo máximo de 5 segundos.");
        } else if self.camera_names.is_empty() {
            ui.label(
                self.camera_error
                    .as_deref()
                    .unwrap_or("Nenhuma câmera disponível. Sem sensor, o Estel seguirá o horário."),
            );
        }
        if ui
            .add_enabled(
                !self.camera_scan_pending && self.calibration_capture.is_none(),
                egui::Button::new("BUSCAR CÂMERAS NOVAMENTE"),
            )
            .clicked()
        {
            self.refresh_cameras();
        }
        ui.label("Mantenha a câmera na posição de uso. Capture primeiro o ambiente com pouca luz e, em outro momento, com mais luz difusa; não cubra a lente nem aponte lâmpadas para ela. Você também pode fazer isso depois no painel, em Luz ambiente.");
        let available = self.calibration_capture.is_none()
            && !self.camera_scan_pending
            && self.selected_camera().is_some()
            && !self.cfg.preserve_colors();
        if ui
            .add_enabled(available, egui::Button::new("1 · CAPTURAR AMBIENTE ESCURO"))
            .clicked()
        {
            self.start_calibration_capture(true);
        }
        if ui
            .add_enabled(
                available && self.calibration_dark.is_some(),
                egui::Button::new("2 · CAPTURAR AMBIENTE CLARO"),
            )
            .clicked()
        {
            self.start_calibration_capture(false);
        }
        if self.calibration_capture.is_some() {
            ui.spinner();
            ui.label("Lendo a câmera… prazo máximo de 5 segundos.");
            if ui.button("CANCELAR LEITURA").clicked() {
                self.cancel_calibration_capture();
            }
        }
        if let Some(message) = &self.calibration_message {
            ui.label(message);
        }
        ui.label(camera_calibration_label(&self.cfg));
        if self.cfg.preserve_colors() {
            ui.label("A preservação de cores pausa os ajustes de tela. Desative-a no painel antes de calibrar a câmera.");
        }
    }

    fn setup_comfort(&mut self, ui: &mut egui::Ui) {
        if time_row(ui, "Acordar", &mut self.wake_h, &mut self.wake_m) {
            self.touch();
        }
        if time_row(ui, "Dormir", &mut self.bed_h, &mut self.bed_m) {
            self.touch();
        }
        ui.label("Escolha limites que mantenham o texto legível e a tela confortável. Os percentuais não medem a luz nos olhos.");
        if ui
            .add(egui::Slider::new(&mut self.cfg.min_brightness, 0.15..=0.80).text("Mínimo"))
            .changed()
        {
            self.touch();
        }
        if ui
            .add(
                egui::Slider::new(
                    &mut self.cfg.day_brightness_max,
                    self.cfg.min_brightness..=1.0,
                )
                .text("Máximo de dia"),
            )
            .changed()
        {
            self.touch();
        }
        self.cfg.rest_brightness_max = self.cfg.rest_brightness_max.clamp(
            self.cfg.min_brightness,
            self.cfg.day_brightness_max.max(self.cfg.min_brightness),
        );
        if ui
            .add(
                egui::Slider::new(
                    &mut self.cfg.rest_brightness_max,
                    self.cfg.min_brightness
                        ..=self.cfg.day_brightness_max.max(self.cfg.min_brightness),
                )
                .text("Máximo à noite / descanso"),
            )
            .changed()
        {
            self.touch();
        }
        if ui
            .checkbox(
                &mut self.cfg.color_critical_work,
                "Preciso preservar as cores para o meu trabalho",
            )
            .changed()
        {
            self.touch();
        }
        if ui
            .checkbox(
                &mut self.cfg.color_vision_deficiency,
                "Tenho daltonismo e prefiro preservar as cores",
            )
            .changed()
        {
            self.touch();
        }
        ui.label("Preservar cores pausa todos os ajustes de tela do Estel. O aplicativo não diagnostica nem corrige daltonismo.");
        if ui
            .checkbox(
                &mut self.cfg.noise_enabled,
                "Quero experimentar som ambiente noturno",
            )
            .changed()
        {
            self.touch();
        }
        ui.label("Som é opcional. Comece baixo e desligue se incomodar. Estes ajustes são preferências de conforto; não são tratamento para ansiedade.");
    }

    fn setup_review(&mut self, ui: &mut egui::Ui) {
        ui.label(format!(
            "Local: {:.4}, {:.4}",
            self.cfg.latitude, self.cfg.longitude
        ));
        ui.label(format!(
            "Janela próxima: {}",
            if self.cfg.window_near { "sim" } else { "não" }
        ));
        ui.label(format!(
            "Direção da janela: {}",
            self.cfg
                .window_azimuth_deg
                .map(|value| format!("{value:.0}°"))
                .unwrap_or_else(|| "não definida".into())
        ));
        ui.label(format!(
            "Luz ambiente: {}",
            if self.cfg.ambient_enabled {
                "ativada; sensor ou câmera com referências válidas"
            } else {
                "desativada"
            }
        ));
        if self.cfg.ambient_enabled {
            ui.label(camera_calibration_label(&self.cfg));
        }
        ui.label(format!(
            "Rotina: {:02}:{:02} até {:02}:{:02}",
            self.wake_h, self.wake_m, self.bed_h, self.bed_m
        ));
        ui.label(format!(
            "Brilho: mínimo {:.0}%, dia até {:.0}%, descanso até {:.0}%",
            self.cfg.min_brightness * 100.0,
            self.cfg.day_brightness_max * 100.0,
            self.cfg.rest_brightness_max * 100.0
        ));
        let mut startup = self.cfg.start_with_windows.unwrap_or(false);
        if ui
            .checkbox(&mut startup, "Iniciar o Estel com o Windows")
            .changed()
        {
            self.cfg.start_with_windows = Some(startup);
            self.touch();
        }
        self.hardware_panel(ui);
        ui.label("Depois de concluir, o Estel fica no ícone ao lado do relógio. Abra o painel por esse ícone para revisar as escolhas, calibrar a câmera ou repetir o guia.");
    }
}

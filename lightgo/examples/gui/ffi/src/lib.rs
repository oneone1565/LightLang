use eframe::egui;
use std::fs;

struct LightApp {
    clicks: u32,
}

impl Default for LightApp {
    fn default() -> Self {
        Self { clicks: 0 }
    }
}

impl eframe::App for LightApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("你好，Light GUI");
            if ui.button("点击").clicked() {
                self.clicks += 1;
            }
            ui.label(format!("点击次数：{}", self.clicks));
        });
    }
}

fn load_cjk_font() -> Option<egui::FontData> {
    [
        "/usr/share/fonts/google-droid-sans-fonts/DroidSansFallbackFull.ttf",
        "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJK-VF.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    ]
    .into_iter()
    .find_map(|path| fs::read(path).ok())
    .map(egui::FontData::from_owned)
}

#[no_mangle]
pub extern "C" fn light_gui_show() -> i64 {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([640.0, 480.0])
            .with_title("Light GUI"),
        ..Default::default()
    };
    match eframe::run_native(
        "Light GUI",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            if let Some(font) = load_cjk_font() {
                fonts.font_data.insert("cjk".to_string(), font.into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "cjk".to_string());
            }
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(LightApp::default()) as Box<dyn eframe::App>)
        }),
    ) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

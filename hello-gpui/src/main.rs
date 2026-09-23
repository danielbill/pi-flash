use gpui::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};

struct HelloWorld {
    text: SharedString,
}

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(0x2d2d2d))
            .size_full()
            .justify_center()
            .items_center()
            .font_family("Segoe UI")
            .text_xl()
            .text_color(rgb(0xffffff))
            .child(format!("Hello, {}!", self.text))
            .child("pi-flash GPUI smoke test — Windows OK")
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .size_8()
                            .bg(gpui::red())
                            .rounded_md()
                            .border_1()
                            .border_dashed()
                            .border_color(gpui::white()),
                    )
                    .child(
                        div()
                            .size_8()
                            .bg(gpui::green())
                            .rounded_md()
                            .border_1()
                            .border_dashed()
                            .border_color(gpui::white()),
                    )
                    .child(
                        div()
                            .size_8()
                            .bg(gpui::blue())
                            .rounded_md()
                            .border_1()
                            .border_dashed()
                            .border_color(gpui::white()),
                    )
                    .child(
                        div()
                            .size_8()
                            .bg(gpui::yellow())
                            .rounded_md()
                            .border_1()
                            .border_dashed()
                            .border_color(gpui::white()),
                    ),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(560.), px(420.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| HelloWorld { text: "World".into() }),
        )
        .unwrap();
        cx.activate(true);
    });
}

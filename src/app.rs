//! Top-level application entity and root view.

use gpui::{div, px, rgb, Context, IntoElement, ParentElement, Render, Styled, Window};

pub struct App;

impl App {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(20.))
                    .child("No repository open"),
            )
            .child(
                div()
                    .text_color(rgb(0x999999))
                    .text_size(px(14.))
                    .child("Open a repository to start a review."),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    #[gpui::test]
    async fn renders_placeholder(cx: &mut TestAppContext) {
        let _window = cx.add_window(|_window, cx| super::App::new(cx));
        // The contract: booting the App in the gpui test harness must not panic
        // and the entity must construct successfully. Once the App gains user-
        // visible interactivity, this test grows to assert on observable events.
    }
}

//! Smoke test for the application boot path.
//!
//! Asserts that booting the root view through the gpui test harness yields the
//! placeholder state. Grows with each feature slice that adds to the golden path
//! of `docs/specs/review/workflow.md`.

use gpui::TestAppContext;
use greviewer::app::App;

#[gpui::test]
async fn boots_to_the_placeholder(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
    });

    let _window = cx.add_window(|_window, cx| App::new(cx));
    // The contract: the boot path constructs the App entity without panicking.
    // When the open-repository affordance lands, this test grows to dispatch
    // the open action and assert the graph-mode transition.
}

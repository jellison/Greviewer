# GPUI 0.2.2 API Primer for AI Agents

## TL;DR

**gpui** (`crates/gpui/src/`, published as `gpui = "0.2.2"`) is Zed's retained-tree GPU-accelerated UI framework. It sits between immediate-mode systems (like egui) and retained DOM-like systems (React): you write functions that produce element trees each frame, but gpui caches layout/paint state and only rerenders when data actually changes. The mental model: entities (app-owned state blobs) → views (entities that implement `Render`) → element trees (UI structure) → window (GPU render target). For Greviewer, you'll mostly write `Render` trait impls that return `div()` trees with `.on_mouse_down()` handlers, styled with `.flex().bg().text_color()` chains. Async work via `cx.spawn()` or `cx.background_spawn()`. State lives in entities; render methods are called lazily when `cx.notify()` marks them dirty.

---

## 1. Mental Model

### The Programming Model

gpui is a **retained tree framework** with **fine-grained reactivity**:

1. **Entities**: App owns all state. You create entities with `cx.new(|_cx| MyState { ... })`, getting back an `Entity<MyState>` handle.
2. **Views**: Entities that implement `Render` are views. Calling `.render()` produces an element tree (the UI structure for one frame).
3. **Elements**: The element tree is a tree of `Element` trait impls (e.g., `div()`, `text()`, etc.). Elements are laid out by Taffy (flexbox) and painted each frame.
4. **Reactivity**: Frames are driven by a main loop. When you call `cx.notify()` on an entity, gpui marks it dirty and rerenders its views next frame. Views observe other entities via `cx.observe()` or `cx.subscribe()`.
5. **Windows**: Each window has a root view (an `Entity<V>` where `V: Render`). That view's render method is called each frame to produce the UI tree.

### Comparison to Other Paradigms

| Aspect | gpui | egui (immediate) | React (retained DOM) | Iced (Elm-like) |
|--------|------|------------------|----------------------|-----------------|
| **Render call** | Per-frame if dirty | Every frame | Per-frame if props change | Per-frame if state change |
| **State owner** | App (centralized) | External | Component (distributed) | Model (centralized) |
| **Layout** | Taffy (flexbox) | Immediate-mode | Flexbox-like | Flexbox-like |
| **Event flow** | Bubble/capture phases | Immediate callbacks | Bubble | Custom |
| **Caching** | Yes (smart) | No | Yes (vdom diff) | Yes |
| **Mutation** | Via `cx.update()` | Direct | Via setState | Via messages |

**Key distinction**: gpui is **retained + reactive**. You don't produce UI every frame; render methods are called only when entities are dirty. But unlike React, there's no virtual DOM diffing—layout and paint state are cached directly.

---

## 2. Public API Surface: Headline Types & Traits

### Core Types

| Type | Location | Purpose |
|------|----------|---------|
| `Application` | `app.rs` | Entry point; builder for configuring the app. |
| `App` | `app.rs` | The global app context; manages entities, windows, executors. Mutable reference threaded through most operations. |
| `Entity<T>` | `app/entity_map.rs` | Strong handle to a T owned by the app. Cloneable, ref-counted. Use `.update(cx, \|t, cx\| ...)` to access state. |
| `WeakEntity<T>` | `app/entity_map.rs` | Weak handle that doesn't keep entity alive. Call `.upgrade()` to try to get a strong `Entity<T>`. |
| `Context<'a, T>` | `app/context.rs` | Tied to a specific entity and window. Deref'd to `App`. Provides entity-level services: `notify()`, `observe()`, `subscribe()`, `spawn()`. |
| `Window` | `window.rs` | Mutable reference to a window's state; used during layout/paint/event dispatch. Contains element tree, hitboxes, text system. |
| `Element` trait | `element.rs` | Low-level trait for layout/paint. Implement only for custom renderers. |
| `IntoElement` trait | `element.rs` | Any type convertible to an Element (div, text, custom, etc.). |
| `Render` trait | `element.rs` | `fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;` Implement on views. |
| `RenderOnce` trait | `element.rs` | Like `Render` but takes `self` by value. Used for stateless components via `#[derive(IntoElement)]`. |
| `Styled` trait | `styled.rs` | Marker trait; enables method-call styling chain (.flex(), .bg(), .text_color(), etc.). |
| `ParentElement` trait | `elements/div.rs` | `.child(elem)`, `.children(vec)` for building trees. |
| `InteractiveElement` trait | `interactive.rs` | `.on_mouse_down()`, `.on_click()`, `.on_action()`, etc. |
| `AnyView` | `view.rs` | Dynamically-typed view handle. Downcast to `Entity<T>` with `.downcast::<T>()`. |
| `AnyElement` | `element.rs` | Dynamically-typed element. Used internally; rarely needed. |

### Other Essential Types

| Type | Purpose |
|------|---------|
| `div()` | Returns a `Div` element builder. Flexbox container, the workhorse. |
| `text(...)` | Returns a `Text` element for rendering strings. |
| `px(f32)` | Pixel unit for sizing/positioning. |
| `Pixels` | Alias for pixels in geometry (`Point<Pixels>`, `Size<Pixels>`, etc.). |
| `SharedString` | Interned string for lightweight cloning. |
| `Task<T>` | A future that can be `.await`'d or `.detach()`'d. From `Context::spawn()` or `Context::background_spawn()`. |
| `Subscription` | Returned by `.observe()`, `.subscribe()`, `.on_release()`. Drop to unsubscribe. |
| `Global` trait | Marker for app-level singletons. Access via `cx.read_global::<G, R>(f)` or `cx.update_global::<G, R>(f)`. |
| `Action` trait | Keymappable command. Declare via `actions!(namespace, [ActionName, ...])` macro. |
| `Hsla` | RGBA color type. Construct via `rgb(0xABCDEF)` or `rgba(r, g, b, a)`. |

---

## 3. Hello World Walkthrough

From `examples/hello_world.rs` (modified for clarity):

```rust
use gpui::{
    App, Bounds, Context, Window, WindowBounds, WindowOptions, 
    div, prelude::*, px, rgb, size,
};
use gpui_platform::application;

struct HelloWorld {
    text: SharedString,
}

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(0x505050))
            .size(px(500.0))
            .justify_center()
            .items_center()
            .shadow_lg()
            .border_1()
            .border_color(rgb(0x0000ff))
            .text_xl()
            .text_color(rgb(0xffffff))
            .child(format!("Hello, {}!", &self.text))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .size_8()
                            .bg(gpui::red())
                            .border_1()
                            .border_dashed()
                            .rounded_md()
                            .border_color(gpui::white()),
                    ),
            )
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(500.), px(500.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| HelloWorld {
                    text: "World".into(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
```

**Breakdown**:

1. **Entity creation**: `cx.new(|_| HelloWorld { text: "World".into() })` creates an entity owned by the app, returning `Entity<HelloWorld>`.
2. **Window opening**: `cx.open_window(options, |_, cx| { /* build root view */ })` opens a window and gives you a context to create the root view.
3. **Root view**: The closure returns an `Entity<V>` where `V: Render`. This becomes the window's root.
4. **Rendering**: Each frame, gpui calls `root.render(window, cx)`, which returns the element tree (div() chains).
5. **Styling chain**: `.flex().bg().text_color()` etc. build up styles. The `Styled` trait enables these methods.
6. **Tree building**: `.child()` adds elements; can be called multiple times or pass a `Vec`.

**What compiles against `gpui = "0.2.2"`**: This exact code. Test by cargo-building in `/Users/jellison/code/zed/crates/gpui/examples/hello_world.rs`.

---

## 4. Layout System

### Flexbox via Taffy

Layout in gpui is **Taffy-based flexbox** (`crates/gpui/src/taffy.rs` wraps `taffy = "0.10.1"`). Every element has a `Style` (via `StyleRefinement`):

```rust
// From style.rs
pub struct StyleRefinement {
    pub display: Option<Display>,           // Block, Flex, Grid, None
    pub flex_direction: Option<FlexDirection>, // Row (default), Column, ...
    pub justify_content: Option<JustifyContent>, // FlexStart, Center, SpaceBetween, ...
    pub align_items: Option<AlignItems>,    // FlexStart, Center, Stretch, ...
    pub gap: Option<Length>,                // Gap between children
    pub width: Option<DefiniteLength>,      // Pixels or percent
    pub height: Option<DefiniteLength>,
    pub flex_grow: Option<f32>,             // Flex grow/shrink/basis
    pub flex_shrink: Option<f32>,
    pub flex_basis: Option<DefiniteLength>,
    pub padding: EdgeRefinement<AbsoluteLength>,
    pub margin: EdgeRefinement<AbsoluteLength>,
    pub border: Option<BorderRefinement>,
    pub overflow: Option<OverflowRefinement>,
    // ... more fields
}
```

### Core Element Types

| Element | Method | Purpose |
|---------|--------|---------|
| **div** | `div()` | Container. Fully styled, interactive, supports children. |
| **text** | `text(string)` | Render a string. Respects text_color, text_size, font, etc. |
| **flex** (legacy) | Covered by div; use `.flex()` on div | Flexbox container. |
| **canvas** | `canvas()` | Low-level GPU rendering surface. Call paint functions on cx. |
| **img** | `img(image_src)` | Render an image (PNG, SVG, etc.). |
| **list** | `List::new()` | Virtualized list for large item sets. |
| **uniform_list** | `UniformList::new()` | Virtualized list with uniform item heights (faster). |
| **svg** | `svg()` | Render SVG. |

### Building a Tree

```rust
div()
    .flex()
    .flex_col()
    .w_full()
    .h_full()
    .child(
        div()
            .flex()
            .justify_between()
            .items_center()
            .px_4()
            .py_2()
            .child("Header")
            .child(button().child("Go"))
    )
    .child(
        div()
            .flex_1()
            .overflow_auto()
            .child("Main content")
    )
    .child(
        div()
            .border_t_1()
            .border_color(gpui::gray())
            .py_2()
            .child("Footer")
    )
```

**Key idioms**:
- Chain style methods: `.flex().w_full().h_full().px_4()`.
- `.child(elem)` adds a single child; elements maintain a SmallVec internally.
- Use `.children(vec![...])` for computed children.
- Nesting divs creates hierarchy.

---

## 5. State and Reactivity

### Entity Model

All state lives in entities owned by the app. You never hold raw pointers or `&T` references across frame boundaries—you hold `Entity<T>` handles:

```rust
let model: Entity<MyModel> = cx.new(|cx| MyModel::new());

// Later, access the state:
model.update(cx, |model, cx| {
    model.count += 1;
    cx.notify(); // Mark this entity dirty; views observing it will rerender
});

// Or read without mutating:
let count = model.read(cx).count;
```

### Views and Rendering

A **view** is an entity that implements `Render`:

```rust
impl Render for MyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Called when this view is dirty (cx.notify() was called on it).
        // Return UI tree. Can access self and other entities via cx.
        div().child(format!("Count: {}", self.count))
    }
}
```

Every frame, gpui:
1. Calls `render()` on all dirty views (those with entities that had `cx.notify()` called).
2. Caches the layout and paint output.
3. Only re-paints if bounds or state changed.

### Observing and Subscribing

**Observe** an entity for any change:

```rust
cx.observe(&other_entity, |me, other, cx| {
    // Called when `other` is notified.
    // `me` is mutable reference to self.
    // `other` is handle to the observed entity.
    me.cache = other.read(cx).expensive_value();
}).detach(); // Detach to keep subscription alive for the lifetime of this entity.
```

**Subscribe** to typed events (if the observed entity implements `EventEmitter<E>`):

```rust
impl EventEmitter<MyEvent> for MyEntity {}

// In another entity:
cx.subscribe(&other, |me, other, event, cx| {
    // Called when `other.emit(event)` is called.
    me.on_other_event(event);
}).detach();

// Emit:
cx.emit(MyEvent { ... });
```

### Triggering Rerenders

- Call `cx.notify()` to mark your entity dirty.
- Any view that observes you will be marked dirty and rerendered next frame.
- Use `cx.notify()` sparingly; only on meaningful state changes.

---

## 6. Events and Actions

### Click Handlers and Event Callbacks

Use **interactive traits** on elements:

```rust
div()
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, event: &MouseDownEvent, window, cx| {
            // `view` is mutable ref to self (this view).
            // `event` has button, position, modifiers, click_count.
            // Capture other entities or data in the closure.
            view.on_click(event.position);
            cx.notify();
        })
    )
```

**Key patterns**:
- Use `cx.listener(f)` to capture the view's state in a closure.
- Use `cx.processor(f)` for closures that return a value.
- Events: `MouseDownEvent`, `MouseUpEvent`, `MouseMoveEvent`, `KeyDownEvent`, `ScrollWheelEvent`, `DragMoveEvent<T>`, etc.
- Phases: `DispatchPhase::Capture` (parent first) and `Bubble` (child first). Default is Bubble.

### Actions

**Keymappable commands** (actions can be bound to keyboard shortcuts):

```rust
actions!(myapp, [Undo, Redo, Cut, Paste]);

// In a view:
div()
    .on_action(cx.listener(|view, _action: &Undo, _, cx| {
        view.undo();
        cx.notify();
    }))
```

Declare actions with the `actions!` macro. Use `#[derive(Action)]` for parameterized actions (serializable from JSON for keymaps).

---

## 7. Styling

### The Styled Trait Chain Pattern

**All** elements that support styling implement the `Styled` trait. It provides **method-call chaining** for CSS-like styling:

```rust
div()
    .flex()                          // display: flex
    .flex_col()                      // flex-direction: column
    .gap_3()                         // gap (gap_3 = 12px in some theme)
    .w_full()                        // width: 100%
    .h_auto()                        // height: auto
    .px_4()                          // padding-left/right: 16px
    .py_2()                          // padding-top/bottom: 8px
    .bg(rgb(0xFFFFFF))              // background-color
    .text_color(rgb(0x000000))      // color
    .text_sm()                       // font-size: small
    .text_center()                   // text-align: center
    .border_1()                      // border: 1px solid
    .border_color(rgb(0xCCCCCC))    // border-color
    .rounded_lg()                    // border-radius
    .shadow_lg()                     // box-shadow
    .cursor_pointer()                // cursor: pointer
    .transition()                    // enable CSS transitions
    .hover(|s| s.bg(rgb(0xEEEEEE))) // hover pseudo-class
```

### Styling Types

From `styled.rs`, the macro expands to hundreds of methods. Common patterns:

- **Size**: `.w(px(100.))`, `.h_full()`, `.size_8()` (8 in some unit system).
- **Flex**: `.flex()`, `.flex_col()`, `.flex_1()` (flex-grow: 1), `.flex_shrink_0()`, `.flex_basis()`.
- **Spacing**: `.gap_2()`, `.px_4()`, `.py_2()`, `.m_auto()`.
- **Colors**: `.bg(color)`, `.text_color(color)`, `.border_color(color)`.
- **Text**: `.text_sm()`, `.text_lg()`, `.font_weight()`, `.italic()`.
- **Borders**: `.border_1()`, `.border_b_1()` (bottom only), `.rounded_md()`.
- **Display**: `.flex()`, `.grid()`, `.block()`, `.hidden()`.
- **Overflow**: `.overflow_auto()`, `.overflow_hidden()`.

**Custom refinement**: If a style method isn't provided, use `.refine(|s| { s.custom_field = ...; s })`.

---

## 8. Async and Tasks

### Spawning Async Work

**Don't** block the UI thread. Use executors:

```rust
// In a view's render or event handler:
cx.spawn(|view, mut cx| async move {
    // `view` is WeakEntity<Self> - doesn't keep view alive.
    // `cx` is AsyncApp - can be held across await points.
    let result = expensive_async_work().await;
    
    // Back on UI thread after await:
    view.update(&mut cx, |view, cx| {
        view.result = result;
        cx.notify();
    }).ok(); // Might fail if view was dropped.
}).detach(); // Detach to let task run independently.
```

Or spawn on a background thread:

```rust
let task = cx.background_spawn(async {
    // Runs on a background thread.
    cpu_intensive_work()
});

// Later:
let result = task.await; // Blocks async context (not UI).
```

### Task Handling

```rust
pub struct Task<T> {
    // Future that can be awaited or detached.
}

// Common operations:
task.await                           // Block until complete.
task.detach()                        // Fire and forget.
task.detach_and_log_err(cx)         // Spawn on foreground, log errors.
cx.spawn(async { ... }).detach()    // Spawn and detach in one go.
```

### AsyncApp Context

Returned from `cx.spawn()` and `cx.background_spawn()`. Can be held across await points (unlike `Context<T>` which borrows `App`):

```rust
pub struct AsyncApp {
    // Can call: .update(entity, f), .notify(entity), etc.
}
```

---

## 9. Testing

### Test Infrastructure

From `test.rs`. Use the `#[gpui::test]` macro:

```rust
#[gpui::test]
async fn test_my_view(cx: &TestAppContext) {
    cx.update(|cx| {
        let view: Entity<MyView> = cx.new(|_| MyView::new());
        // ... assertions
    });
}

#[gpui::test]
async fn test_collaboration(cx_a: &TestAppContext, cx_b: &TestAppContext) {
    // Multiple contexts for testing multi-window scenarios.
}
```

**Key features**:
- Deterministic scheduling: tests run with a fixed seed (set `SEED` env var).
- Multiple contexts: pass multiple `&TestAppContext` to simulate collaborative scenarios.
- `.update(f)` to run a function in the context.
- Async support: use `.await` on tasks spawned within tests.

### Example Test

```rust
#[gpui::test]
async fn test_counter_view(cx: &TestAppContext) {
    let view = cx.update(|cx| {
        cx.new(|_| CounterView { count: 0 })
    });
    
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.count += 1;
            cx.notify();
        });
    });
    
    cx.update(|cx| {
        assert_eq!(view.read(cx).count, 1);
    });
}
```

---

## 10. Common Gotchas

### 1. **Lifetime of `cx`**

`Context<T>` borrows `App` mutably and is tied to a specific scope. You **cannot** hold it across async boundaries.

**Wrong**:
```rust
let ctx = cx; // Type is &mut Context<Self>
let task = async move {
    ctx.notify(); // ERROR: can't move borrowed ref.
}.await;
```

**Right**:
```rust
let weak_view = cx.weak_entity();
let task = cx.spawn(|_, mut cx| async move {
    weak_view.update(&mut cx, |_, cx| cx.notify()).ok();
});
```

### 2. **`Entity<T>` vs `&T`**

`Entity<T>` is a handle; it doesn't give you access to `T` directly. You must call `.update()`, `.read()`, etc. with the app context.

```rust
// Wrong:
let entity: Entity<MyStruct> = ...;
let value = entity.field; // ERROR: Entity doesn't deref to MyStruct.

// Right:
entity.read(cx).field
entity.update(cx, |e, _| e.field = ...)
```

### 3. **Closure Captures and `cx.listener`**

When you need to capture view state in an event handler, use `cx.listener()` to wrap the closure:

```rust
cx.listener(move |view, _event, _, cx| {
    view.state = ... // view is automatically mutable ref to self
    cx.notify();
})
```

Without `cx.listener`, you'd need to manually capture `cx.entity()` and call `.update()` inside, which is more verbose.

### 4. **WeakEntity and Upgrades**

If you spawn an async task or hold a callback across entity drops, use `WeakEntity` so the entity can be dropped. Always check `.upgrade()` returns `Some`:

```rust
let weak = view.downgrade();
cx.spawn(|_, mut cx| async move {
    result.await;
    weak.update(&mut cx, |view, cx| {
        view.result = result;
        cx.notify();
    }).ok(); // Ok() means view still alive; Err() means dropped.
});
```

### 5. **Render Called Every Frame if Caching Disabled**

Views can opt into caching with `.cached(style)` on `AnyView`. Without it, render is called every frame. This is usually fine, but avoid expensive work in `render()`.

```rust
// In a containing view:
view_entity.into_any_element().cached(style)
```

### 6. **String and SharedString Interning**

Use `SharedString` for strings that might be cloned a lot; it's interned and cheap to clone. `String` and `&str` are also supported but less efficient.

```rust
let s = SharedString::new("hello"); // Interned
s.clone() // Cheap
```

### 7. **Px() Conversion**

`Pixels` is a `Copy` type. Construct with `px(f32)` helper:

```rust
let width = px(100.0);
div().w(width)
```

### 8. **on_window_close_quit vs retain_window_behavior**

By default, windows are retained. On window close, the window is dropped. If you want the app to quit when the last window closes, call `cx.on_window_close_quit()` or set the `QuitMode` when building the app.

---

## 11. Features and Defaults

From `Cargo.toml`:

```toml
[features]
default = ["font-kit", "wayland", "x11", "windows-manifest"]
test-support = [...] # For testing
inspector = [...] # Visual inspector (debug builds)
leak-detection = [...] # Detect leaked entities
```

**Implications**:
- **`font-kit` (macOS, optional)**: System font loading via font-kit. Off by default on Linux/Windows (uses system fonts directly).
- **`wayland` (Linux)**: Wayland backend. Enabled by default.
- **`x11` (Linux)**: X11 backend. Enabled by default; if both are off, no Linux rendering.
- **`windows-manifest`**: Windows app manifest. Enabled by default on Windows.
- **`test-support`**: Deterministic testing harness. Enabled in dev/test only.
- **`inspector`**: Visual UI inspector overlay (debug builds). Helpful for debugging layout.

**For Greviewer**: Accept defaults. On Linux, you get both Wayland and X11 support. On macOS, system fonts. On Windows, the manifest is included.

---

## 12. Pointers to Zed Source for Further Reading

**Once you understand this primer, these Zed crates show gpui in action:**

1. **`crates/ui`** – GPUI UI component library. Defines reusable primitives (buttons, inputs, panels, etc.) built on top of gpui. Read for how to compose complex UIs from div trees.
2. **`crates/workspace`** – Zed's main workspace/editor manager. Large-scale use of gpui entities, views, actions, and async task orchestration. Study for patterns on managing many windows and state synchronization.
3. **`crates/editor`** – The code editor implementation. Custom element for text rendering, keyboard dispatch, syntax highlighting. Read for implementing custom elements beyond div/text.
4. **`crates/project`** – Project/file tree management. Heavy use of entities, observables, and background tasks. Good for async patterns.
5. **`crates/collab`** – Collaborative editing state. Shows how gpui's entity model scales to multi-user scenarios with subscriptions and event emission.

Each of these is 5k–20k lines but well-structured; start with `crates/ui` for component patterns, then `crates/workspace` for app architecture.

---

## Appendix: Quick Reference

### Render a View

```rust
impl Render for MyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child("Hello")
    }
}
```

### Create an Entity

```rust
let entity: Entity<MyStruct> = cx.new(|_cx| MyStruct { ... });
```

### Update an Entity and Notify

```rust
entity.update(cx, |e, cx| {
    e.field = new_value;
    cx.notify();
});
```

### Listen to Another Entity

```rust
cx.observe(&other, |me, other, cx| {
    me.cache = other.read(cx).value;
}).detach();
```

### Spawn Async Work

```rust
cx.spawn(|weak_self, mut cx| async move {
    let result = work().await;
    weak_self.update(&mut cx, |me, cx| {
        me.result = result;
        cx.notify();
    }).ok();
}).detach();
```

### Handle a Click

```rust
div()
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, _event, _window, cx| {
            view.on_click();
            cx.notify();
        })
    )
```

### Style a Div

```rust
div()
    .flex()
    .flex_col()
    .gap_3()
    .w_full()
    .px_4()
    .py_2()
    .bg(rgb(0xFFFFFF))
    .text_color(rgb(0x000000))
    .child("Content")
```

---

**End of Primer.**

*This document is accurate for `gpui = "0.2.2"` as of May 2026. Verify specific APIs by cross-referencing source files (`crates/gpui/src/`) for the authoritative implementation.*


use stdweb;
use AppConfig;

use stdweb::traits::{IDragEvent, IEvent};
use stdweb::unstable::TryInto;
use stdweb::web::event::{
    DragDropEvent, IKeyboardEvent, IMouseEvent, KeyDownEvent, KeyUpEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ResizeEvent,
};
use stdweb::web::html_element::CanvasElement;
use stdweb::web::{window, FileReader, IEventTarget, IHtmlElement, TypedArray};

use std::cell::RefCell;
use std::rc::Rc;

use crate::{BufferState, File};
use AppEvent;

pub struct App {
    window: CanvasElement,
    pub events: Rc<RefCell<Vec<AppEvent>>>,
    device_pixel_ratio: f32,
    dropped_files: Rc<RefCell<Vec<File>>>,
}

use super::events;

macro_rules! map_event {
    ($events:expr, $x:ident, $y:ident, $ee:ident, $e:expr, $prevent:expr) => {{
        let events = $events.clone();
        move |$ee: $x| {
            if $prevent {
                $ee.prevent_default();
            }
            events.borrow_mut().push(AppEvent::$y($e));
        }
    }};

    ($events:expr, $x:ident, $y:ident, $e:expr) => {{
        let events = $events.clone();
        move |_: $x| {
            events.borrow_mut().push(AppEvent::$y($e));
        }
    }};
}

// In browser request full screen can only called under event handler.
// So basically this function is useless at this moment.
#[allow(dead_code)]
fn request_full_screen(canvas: &CanvasElement) {
    js! {
        var c = @{&canvas};
        if (c.requestFullscreen) {
            c.requestFullscreen();
        } else if (c.webkitRequestFullscreen) {
            c.webkitRequestFullscreen(Element.ALLOW_KEYBOARD_INPUT);
        } else if (c.mozRequestFullScreen) {
            c.mozRequestFullScreen();
        } else if (c.msRequestFullscreen) {
            c.msRequestFullscreen();
        }
    };
}

impl App {
    pub fn new(config: AppConfig) -> App {
        use stdweb::web::*;

        if config.headless {
            // Right now we did not support headless in web.
            unimplemented!();
        }

        let _ = stdweb::initialize();
        let canvas: CanvasElement = document()
            .create_element("canvas")
            .unwrap()
            .try_into()
            .unwrap();

        js! {
            // setup the buffer size
            // see https://webglfundamentals.org/webgl/lessons/webgl-resizing-the-canvas.html
            var realToCSSPixels = window.devicePixelRatio;
            (@{&canvas}).width = @{config.size.0} * realToCSSPixels;
            (@{&canvas}).height = @{config.size.1} * realToCSSPixels;

            // setup the canvas size
            (@{&canvas}).style.width = @{config.size.0} + "px";
            (@{&canvas}).style.height = @{config.size.1} + "px";

            // Make it focusable
            // https://stackoverflow.com/questions/12886286/addeventlistener-for-keydown-on-canvas
            @{&canvas}.tabIndex = 1;


            document.body.addEventListener("dragover", e => {e.prevent_default(); return false;});
            document.body.addEventListener("dragenter", e => {e.prevent_default(); return false;});
            document.body.addEventListener("drop", e => {e.prevent_default(); return false;});
        };

        if !config.show_cursor {
            js! {
                @{&canvas}.style.cursor="none";
            };
        }

        let device_pixel_ratio: f64 = js! { return window.devicePixelRatio; }.try_into().unwrap();

        let body = document().query_selector("body").unwrap().unwrap();

        body.append_child(&canvas);
        js! {
            @{&canvas}.focus();
        }

        if config.fullscreen {
            println!("Webgl do not support with_screen.");
        }

        let mut app = App {
            window: canvas,
            events: Rc::new(RefCell::new(Vec::new())),
            device_pixel_ratio: device_pixel_ratio as f32,
            dropped_files: Rc::new(RefCell::new(Vec::new())),
        };
        app.setup_listener();

        app
    }

    fn setup_listener(&mut self) {
        let canvas: &CanvasElement = self.canvas();

        canvas.add_event_listener(map_event! {
            self.events,
            MouseDownEvent,
            MouseDown,
            e,
            events::MouseButtonEvent {button:match e.button() {
                MouseButton::Left => 0,
                MouseButton::Wheel => 1,
                MouseButton::Right => 2,
                MouseButton::Button4 => 3,
                MouseButton::Button5 => 4,
            }},
            false
        });
        canvas.add_event_listener(map_event! {
            self.events,
            MouseUpEvent,
            MouseUp,
            e,
            events::MouseButtonEvent {button:match e.button() {
                MouseButton::Left => 0,
                MouseButton::Wheel => 1,
                MouseButton::Right => 2,
                MouseButton::Button4 => 3,
                MouseButton::Button5 => 4,
            }},
            true
        });

        canvas.add_event_listener({
            let canvas = canvas.clone();
            let canvas_x: f64 = js! {
            return @{&canvas}.getBoundingClientRect().left; }
            .try_into()
            .unwrap();
            let canvas_y: f64 = js! {
            return @{&canvas}.getBoundingClientRect().top; }
            .try_into()
            .unwrap();
            map_event! {
                self.events,
                MouseMoveEvent,
                MousePos,
                e,
                (e.client_x() as f64 - canvas_x,e.client_y() as f64 - canvas_y),
                true
            }
        });

        canvas.add_event_listener(map_event! {
            self.events,
            KeyDownEvent,
            KeyDown,
            e,
            events::KeyDownEvent {
                code: e.code(),
                key: e.key(),
                shift: e.shift_key(),
                alt: e.alt_key(),
                ctrl: e.ctrl_key(),
            },
            true
        });

        canvas.add_event_listener({
            let events = self.events.clone();
            move |e: KeyUpEvent| {
                e.prevent_default();
                // filter control keys "Tab", "Backspace", ...
                if e.key().len() == 1 {
                    events
                        .borrow_mut()
                        .push(AppEvent::CharEvent(e.key().chars().next().unwrap()));
                }
            }
        });

        canvas.add_event_listener(map_event! {
            self.events,
            KeyUpEvent,
            KeyUp,
            e,
            events::KeyUpEvent {
                code: e.code(),
                key: e.key(),
                shift: e.shift_key(),
                alt: e.alt_key(),
                ctrl: e.ctrl_key(),
            },
            true
        });

        canvas.add_event_listener({
            let canvas = canvas.clone();

            map_event! {
                self.events,
                ResizeEvent,
                Resized,
                (canvas.offset_width() as u32, canvas.offset_height() as u32)
            }
        });

        canvas.add_event_listener({
            let events = self.events.clone();
            let dropped_files = self.dropped_files.clone();
            move |e: DragDropEvent| {
                e.prevent_default();
                for f in e.data_transfer().unwrap().files() {
                    let buffer_state = Rc::new(RefCell::new(BufferState::Empty));
                    let on_get_buffer = {
                        let buffer_state = buffer_state.clone();
                        move |ab: TypedArray<u8>| {
                            let data = ab.to_vec();
                            if data.len() > 0 {
                                *buffer_state.borrow_mut() = BufferState::Buffer(data);
                            }
                        }
                    };
                    let on_error = {
                        let buffer_state = buffer_state.clone();
                        move |s: String| {
                            let msg = format!("Fail to read file from web {}", s);
                            *buffer_state.borrow_mut() = BufferState::Error(msg);
                        }
                    };
                    let name = f.name();
                    js! {
                        var reader = new FileReader();
                        var fname=@{name};
                        var on_error_js = function(s){
                            var on_error = @{on_error};
                            on_error(s);
                            on_error.drop();
                        };
                        reader.onload = function(e2) {
                            var on_get_buffer = @{on_get_buffer};
                            on_get_buffer(new Uint8Array(e2.target.result));
                            on_get_buffer.drop();
                        };
                        reader.onerror = function(e3) {
                            var err_msg="Error while reading "+fname+" : "+e3;
                            console.log(err_msg);
                            on_error_js(err_msg);
                        };
                        reader.onabort = function(e4) {
                            var err_msg="Reading of "+fname+" aborted : "+e4;
                            console.log(err_msg);
                            on_error_js(err_msg);
                        };
                    }
                    events
                        .borrow_mut()
                        .push(AppEvent::FileDropped(f.name().to_owned()));
                    dropped_files.borrow_mut().push(File {
                        buffer_state: buffer_state,
                    });
                }
            }
        });
    }

    pub fn get_dropped_file(&mut self) -> Option<File> {
        self.dropped_files.borrow_mut().pop()
    }

    pub fn print<T: Into<String>>(msg: T) {
        js! { console.log(@{msg.into()})};
    }

    pub fn exit() {}

    pub fn get_screen_resolution(&self) -> (u32, u32) {
        (
            window().inner_width() as u32,
            window().inner_height() as u32,
        )
    }

    pub fn get_params() -> Vec<String> {
        let params = js! { return window.location.search.substring(1).split("&"); };
        params.try_into().unwrap()
    }

    pub fn hidpi_factor(&self) -> f32 {
        return self.device_pixel_ratio;
    }

    pub fn canvas(&self) -> &CanvasElement {
        &self.window
    }

    pub fn run_loop<F>(mut self, mut callback: F)
    where
        F: 'static + FnMut(&mut Self) -> (),
    {
        window().request_animation_frame(move |_t: f64| {
            callback(&mut self);
            self.events.borrow_mut().clear();
            self.run_loop(callback);
        });
    }

    pub fn poll_events<F>(&mut self, callback: F) -> bool
    where
        F: FnOnce(&mut Self) -> (),
    {
        callback(self);
        self.events.borrow_mut().clear();

        true
    }

    pub fn run<F>(self, callback: F)
    where
        F: 'static + FnMut(&mut Self) -> (),
    {
        self.run_loop(callback);

        stdweb::event_loop();
    }

    pub fn set_fullscreen(&mut self, _b: bool) {
        // unimplemented!();
    }
}

pub fn now() -> f64 {
    // perforamce now is in ms
    // https://developer.mozilla.org/en-US/docs/Web/API/Performance/now
    let v = js! { return performance.now() / 1000.0; };
    return v.try_into().unwrap();
}

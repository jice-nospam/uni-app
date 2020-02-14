mod native_keycode;

use glutin;
use glutin::event::{ElementState, Event, MouseButton, WindowEvent};
use std::cell::RefCell;
use std::env;
use std::os::raw::c_void;
use std::process;
use std::rc::Rc;
use time;

use AppConfig;
use AppEvent;

use self::native_keycode::{translate_scan_code, translate_virtual_key};
use super::events;

enum WindowContext {
    Normal(glutin::WindowedContext<glutin::PossiblyCurrent>),
    Headless(glutin::Context<glutin::NotCurrent>),
}

impl WindowContext {
    fn hidpi_factor(&self) -> f64 {
        match self {
            WindowContext::Normal(ref w) => w.window().scale_factor(),
            _ => 1.0,
        }
    }

    fn swap_buffers(&self) -> Result<(), glutin::ContextError> {
        match self {
            WindowContext::Normal(ref w) => w.swap_buffers(),
            WindowContext::Headless(_) => Ok(()),
        }
    }
}

/// the main application struct
pub struct App {
    window: WindowContext,
    events_loop: glutin::event_loop::EventLoop<()>,
    exiting: bool,
    modifiers: glutin::event::ModifiersState,
    pub events: Rc<RefCell<Vec<AppEvent>>>,
    config: AppConfig,
    monitor: glutin::monitor::MonitorHandle,
}

fn get_virtual_key(input: glutin::event::KeyboardInput) -> String {
    match input.virtual_keycode {
        Some(k) => {
            let mut s = translate_virtual_key(k).into();
            if s == "" {
                s = format!("{:?}", k);
            }
            s
        }
        None => "".into(),
    }
}

fn get_scan_code(input: glutin::event::KeyboardInput) -> String {
    translate_scan_code(input.scancode).into()
}

fn translate_event(
    e: glutin::event::Event<()>,
    modifiers: glutin::event::ModifiersState,
) -> Option<AppEvent> {
    if let Event::WindowEvent {
        event: winevent, ..
    } = e
    {
        match winevent {
            WindowEvent::MouseInput { state, button, .. } => {
                let button_num = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    MouseButton::Other(val) => val as usize,
                };
                let event = events::MouseButtonEvent { button: button_num };
                match state {
                    ElementState::Pressed => Some(AppEvent::MouseDown(event)),
                    ElementState::Released => Some(AppEvent::MouseUp(event)),
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                Some(AppEvent::MousePos((position.x, position.y)))
            }
            WindowEvent::KeyboardInput { input, .. } => match input.state {
                ElementState::Pressed => Some(AppEvent::KeyDown(events::KeyDownEvent {
                    key: get_virtual_key(input),
                    code: get_scan_code(input),
                    shift: modifiers.shift(),
                    alt: modifiers.alt(),
                    ctrl: modifiers.ctrl(),
                })),
                ElementState::Released => Some(AppEvent::KeyUp(events::KeyUpEvent {
                    key: get_virtual_key(input),
                    code: get_scan_code(input),
                    shift: modifiers.shift(),
                    alt: modifiers.alt(),
                    ctrl: modifiers.ctrl(),
                })),
            },
            WindowEvent::ReceivedCharacter(c) => Some(AppEvent::CharEvent(c)),
            WindowEvent::Resized(size) => Some(AppEvent::Resized(size.into())),
            WindowEvent::CloseRequested => Some(AppEvent::CloseRequested),
            _ => None,
        }
    } else {
        None
    }
}

fn gl_version() -> glutin::GlRequest {
    glutin::GlRequest::GlThenGles {
        opengl_version: (3, 2),
        opengles_version: (2, 0),
    }
}

fn new_headless_context(
    config: &AppConfig,
    event_loop: &glutin::event_loop::EventLoop<()>,
) -> WindowContext {
    let context = glutin::ContextBuilder::new()
        .with_gl(gl_version())
        .with_gl_profile(glutin::GlProfile::Core)
        .build_headless(
            event_loop,
            glutin::dpi::PhysicalSize::new(config.size.0, config.size.1),
        )
        .unwrap();

    WindowContext::Headless(context)
}

fn new_fullscreen_context(
    config: &AppConfig,
    event_loop: &glutin::event_loop::EventLoop<()>,
    video_mode: glutin::monitor::VideoMode,
) -> WindowContext {
    let window_builder = glutin::window::WindowBuilder::new()
        .with_title(&config.title)
        .with_fullscreen(Some(glutin::window::Fullscreen::Exclusive(video_mode)))
        .into();
    let context = glutin::ContextBuilder::new()
        .with_vsync(config.vsync)
        .with_gl(gl_version())
        .with_gl_profile(glutin::GlProfile::Core)
        .build_windowed(window_builder, event_loop)
        .unwrap();
    if !config.show_cursor {
        context.window().set_cursor_visible(false);
    }
    let context = unsafe { context.make_current() }.unwrap();
    WindowContext::Normal(context)
}

fn new_windowed_context(
    config: &AppConfig,
    event_loop: &glutin::event_loop::EventLoop<()>,
) -> WindowContext {
    let scale_factor = event_loop.primary_monitor().scale_factor();
    let window_builder = glutin::window::WindowBuilder::new()
        .with_title(&config.title)
        .with_resizable(config.resizable)
        .with_inner_size(glutin::dpi::PhysicalSize::<u32>::from((
            (f64::from(config.size.0) * scale_factor) as u32,
            (f64::from(config.size.1) * scale_factor) as u32,
        )))
        .into();

    let context = glutin::ContextBuilder::new()
        .with_vsync(config.vsync)
        .with_gl(gl_version())
        .with_gl_profile(glutin::GlProfile::Core)
        .build_windowed(window_builder, event_loop)
        .unwrap();

    if !config.show_cursor {
        context.window().set_cursor_visible(false);
    }
    let context = unsafe { context.make_current() }.unwrap();

    WindowContext::Normal(context)
}

fn find_fullscreen_mode(
    monitor: &glutin::monitor::MonitorHandle,
    config: &AppConfig,
) -> Option<glutin::monitor::VideoMode> {
    let mut video_mode = None;
    let mut smallest = 40000 * 40000;
    for mode in monitor.video_modes() {
        let size = mode.size();
        let surf = size.width * size.height;
        if size.width >= config.size.0 && size.height >= config.size.1 && surf < smallest {
            video_mode = Some(mode);
            smallest = surf;
        }
    }
    video_mode
}

impl App {
    /// create a new game window
    pub fn new(config: AppConfig) -> App {
        let events_loop = glutin::event_loop::EventLoop::new();
        let monitor = events_loop.primary_monitor();

        let window = if config.headless {
            new_headless_context(&config, &events_loop)
        } else if config.fullscreen {
            if let Some(video_mode) = find_fullscreen_mode(&monitor, &config) {
                new_fullscreen_context(&config, &events_loop, video_mode)
            } else {
                new_windowed_context(&config, &events_loop)
            }
        } else {
            new_windowed_context(&config, &events_loop)
        };

        App {
            window,
            events_loop,
            exiting: false,
            events: Rc::new(RefCell::new(Vec::new())),
            modifiers: Default::default(),
            config,
            monitor,
        }
    }

    /// return the screen resolution in physical pixels
    pub fn get_screen_resolution(&self) -> (u32, u32) {
        if let WindowContext::Normal(ref glwindow) = self.window {
            glwindow.window().current_monitor().size().into()
        } else {
            (0, 0)
        }
    }

    /// return the command line / URL parameters
    pub fn get_params() -> Vec<String> {
        let mut params: Vec<String> = env::args().collect();
        params.remove(0);
        params
    }

    /// activate or deactivate fullscreen. only works on native target
    pub fn set_fullscreen(&self, b: bool) {
        if let WindowContext::Normal(ref glwindow) = self.window {
            if b {
                if let Some(video_mode) = find_fullscreen_mode(&self.monitor, &self.config) {
                    glwindow
                        .window()
                        .set_fullscreen(Some(glutin::window::Fullscreen::Exclusive(video_mode)));
                }
            } else {
                glwindow.window().set_fullscreen(None);
            }
        }
    }

    /// print a message on standard output (native) or js console (web)
    pub fn print<T: Into<String>>(msg: T) {
        print!("{}", msg.into());
    }

    /// exit current process (close the game window). On web target, this does nothing.
    pub fn exit() {
        process::exit(0);
    }

    /// returns the HiDPI factor for current screen
    pub fn hidpi_factor(&self) -> f64 {
        self.window.hidpi_factor()
    }

    fn get_proc_address(&self, name: &str) -> *const c_void {
        if let WindowContext::Normal(ref glwindow) = self.window {
            glwindow.get_proc_address(name) as *const c_void
        } else {
            unreachable!()
        }
    }

    /// return the opengl context for this window
    pub fn canvas<'p>(&'p self) -> Box<dyn 'p + FnMut(&str) -> *const c_void> {
        Box::new(move |name| self.get_proc_address(name))
    }

    fn handle_event(&mut self, event: glutin::event::Event<()>) -> bool {
        let mut running = true;

        let (window, events) = (&self.window, &mut self.events);
        let intercept_close_request = self.config.intercept_close_request;
        match event {
            glutin::event::Event::WindowEvent { ref event, .. } => match event {
                &glutin::event::WindowEvent::CloseRequested => {
                    if !intercept_close_request {
                        running = false;
                    }
                }
                &glutin::event::WindowEvent::Resized(size) => {
                    // Fixed for Windows which minimized to emit a Resized(0,0) event
                    if size.width != 0 && size.height != 0 {
                        if let WindowContext::Normal(glwindow) = window {
                            glwindow.resize(size);
                        }
                    }
                }
                &glutin::event::WindowEvent::KeyboardInput { input, .. } => {
                    // issue tracked in https://github.com/tomaka/winit/issues/41
                    // Right now we handle it manually.
                    if cfg!(target_os = "macos") {
                        if let Some(keycode) = input.virtual_keycode {
                            if keycode == glutin::event::VirtualKeyCode::Q && self.modifiers.logo()
                            {
                                running = false;
                            }
                        }
                    }
                }
                _ => (),
            },
            _ => (),
        };
        translate_event(event, self.modifiers).map(|evt| events.borrow_mut().push(evt));

        return running;
    }

    /// start the game loop, calling provided callback every frame
    pub fn run<'a, F: 'static>(mut self, mut callback: F)
    where
        F: FnMut(&mut Self) -> (),
    {
        let events_loop =
            std::mem::replace(&mut self.events_loop, glutin::event_loop::EventLoop::new());
        events_loop.run(move |event, _window_target, control_flow| {
            if !self.handle_event(event) {
                *control_flow = glutin::event_loop::ControlFlow::Exit;
            } else {
                callback(&mut self);
                self.events.borrow_mut().clear();
                self.window.swap_buffers().unwrap();

                if self.exiting {
                    *control_flow = glutin::event_loop::ControlFlow::Exit;
                }
            }
        });
    }
}

/// return the time since the start of the program in seconds
pub fn now() -> f64 {
    // precise_time_s() is in second
    // https://doc.rust-lang.org/time/time/fn.precise_time_s.html
    time::precise_time_s()
}

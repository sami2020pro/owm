// Copied and adapted from x11rb (https://github.com/psychon/x11rb/)

/*
 * Dear future guys.
 * Please forgive me.
 * I can't even begin to
 * express how sorry I am!
 * For making this trash...
 *
 * - Sam Ghasemi
 */

extern crate x11rb;

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::process::exit;

use x11rb::connection::Connection;
use x11rb::errors::{ReplyError, ReplyOrIdError};
use x11rb::protocol::xproto::*;
use x11rb::protocol::{ErrorKind, Event};
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};

const TITLEBAR_HEIGHT: u16 = 20;
// NOTE: This font may not be available on the some systems.
const DEFAULT_FONT: &str = "6x13";
const DRAG_BUTTON: Button = 1;
const SPACE: i32 = 3;

#[derive(Debug)]
struct WindowState {
    window: Window,
    frame_window: Window,
    x: i16,
    y: i16,
    width: u16,
}

impl WindowState {
    fn new(window: Window, frame_window: Window, geom: &GetGeometryReply) -> WindowState {
        WindowState {
            window,
            frame_window,
            x: geom.x,
            y: geom.y,
            width: geom.width,
        }
    }

    fn close_area(&self) -> i16 {
        std::cmp::max(0, self.width - TITLEBAR_HEIGHT) as _
    }
}

#[derive(Debug)]
struct WmState<'a, C: Connection> {
    conn: &'a C,
    screen_num: usize,
    black_gc: Gcontext,
    windows: Vec<WindowState>,
    pending_expose: HashSet<Window>,
    wm_protocols: Atom,
    wm_delete_window: Atom,
    sequences_to_ignore: BinaryHeap<Reverse<u16>>,
    drag_window: Option<(Window, (i16, i16))>,
    font_ascent: i16,
    font_descent: i16,
}

impl<'a, C: Connection> WmState<'a, C> {
    fn new(conn: &'a C, screen_num: usize) -> Result<WmState<'a, C>, ReplyOrIdError> {
        let screen = &conn.setup().roots[screen_num];
        let black_gc = conn.generate_id()?;
        let font = conn.generate_id()?;

        conn.open_font(font, DEFAULT_FONT.as_bytes())?;

        let font_information = conn.query_font(font)?.reply()?;

        let gc_aux = CreateGCAux::new()
            .graphics_exposures(0)
            .background(screen.white_pixel)
            .foreground(screen.black_pixel)
            .font(font);
        conn.create_gc(black_gc, screen.root, &gc_aux)?;
        conn.close_font(font)?;

        let wm_protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?;
        let wm_delete_window = conn.intern_atom(false, b"WM_DELETE_WINDOW")?;

        Ok(WmState {
            conn,
            screen_num,
            black_gc,
            windows: Vec::default(),
            pending_expose: HashSet::default(),
            wm_protocols: wm_protocols.reply()?.atom,
            wm_delete_window: wm_delete_window.reply()?.atom,
            sequences_to_ignore: Default::default(),
            drag_window: None,
            font_ascent: font_information.font_ascent,
            font_descent: font_information.font_descent,
        })
    }

    fn scan_windows(&mut self) -> Result<(), ReplyOrIdError> {
        let screen = &self.conn.setup().roots[self.screen_num];
        let tree_reply = self.conn.query_tree(screen.root)?.reply()?;

        let mut cookies = Vec::with_capacity(tree_reply.children.len());
        for win in tree_reply.children {
            let attr = self.conn.get_window_attributes(win)?;
            let geom = self.conn.get_geometry(win)?;
            cookies.push((win, attr, geom));
        }
        for (win, attr, geom) in cookies {
            let (attr, geom) = (attr.reply(), geom.reply());
            if attr.is_err() || geom.is_err() {
                continue;
            }
            let (attr, geom) = (attr.unwrap(), geom.unwrap());
            if !attr.override_redirect && attr.map_state != MapState::UNMAPPED {
                self.manage_window(win, &geom)?;
            }
        }

        Ok(())
    }

    fn manage_window(
        &mut self,
        win: Window,
        geom: &GetGeometryReply,
    ) -> Result<(), ReplyOrIdError> {
        let screen = &self.conn.setup().roots[self.screen_num];
        assert!(self.find_window_by_id(win).is_none());

        let frame_win = self.conn.generate_id()?;

        let win_aux = CreateWindowAux::new()
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::ENTER_WINDOW
                    | EventMask::KEY_PRESS
                    | EventMask::KEY_RELEASE,
            )
            .background_pixel(16776960);

        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            frame_win,
            screen.root,
            geom.x,
            geom.y,
            geom.width,
            geom.height + TITLEBAR_HEIGHT,
            1,
            WindowClass::INPUT_OUTPUT,
            0,
            &win_aux,
        )?;

        self.conn.grab_server()?;
        self.conn.change_save_set(SetMode::INSERT, win)?;
        let cookie = self
            .conn
            .reparent_window(win, frame_win, 0, TITLEBAR_HEIGHT as _)?;
        
        self.conn.map_window(win)?;

        self.conn.map_window(frame_win)?;

        self.conn.ungrab_server()?;

        self.windows.push(WindowState::new(win, frame_win, geom));

        self.sequences_to_ignore
            .push(Reverse(cookie.sequence_number() as u16));
        Ok(())
    }

    fn redraw_titlebar(&self, state: &WindowState) -> Result<(), ReplyError> {
        let x = state.width as i32 - (self.bar_height() / 2) - 10;
        let topleft_offset = (self.bar_height() / 2) - 5;

        self.conn.poly_line(
            CoordMode::ORIGIN,
            state.frame_window,
            self.black_gc,
            &[
                Point {
                    x: (x + topleft_offset + 1) as i16,
                    y: topleft_offset as i16,
                },
                Point {
                    x: (x + topleft_offset + 8) as i16,
                    y: (topleft_offset + 7) as i16,
                },
                Point {
                    x: (x + topleft_offset + 1) as i16,
                    y: (topleft_offset + 1) as i16,
                },
                Point {
                    x: (x + topleft_offset + 7) as i16,
                    y: (topleft_offset + 7) as i16,
                },
                Point {
                    x: (x + topleft_offset) as i16,
                    y: (topleft_offset + 1) as i16,
                },
                Point {
                    x: (x + topleft_offset + 7) as i16,
                    y: (topleft_offset + 8) as i16,
                },
            ],
        )?;

        self.conn.poly_line(
            CoordMode::ORIGIN,
            state.frame_window,
            self.black_gc,
            &[
                Point {
                    x: (x + topleft_offset) as i16,
                    y: (topleft_offset + 7) as i16,
                },
                Point {
                    x: (x + topleft_offset + 7) as i16,
                    y: topleft_offset as i16,
                },
                Point {
                    x: (x + topleft_offset + 1) as i16,
                    y: (topleft_offset + 7) as i16,
                },
                Point {
                    x: (x + topleft_offset + 7) as i16,
                    y: (topleft_offset + 1) as i16,
                },
                Point {
                    x: (x + topleft_offset + 1) as i16,
                    y: (topleft_offset + 8) as i16,
                },
                Point {
                    x: (x + topleft_offset + 8) as i16,
                    y: (topleft_offset + 1) as i16,
                },
            ],
        )?;

        let reply = self
            .conn
            .get_property(
                false,
                state.window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                0,
                std::u32::MAX,
            )?
            .reply()?;

        self.conn.image_text8(
            state.frame_window,
            self.black_gc,
            SPACE as i16,
            SPACE as i16 + self.font_ascent,
            &reply.value,
        )?;

        Ok(())
    }

    fn bar_height(&self) -> i32 {
        self.font_ascent as i32 + self.font_descent as i32 + 2 * SPACE + 2
    }

    fn refresh(&mut self) {
        while let Some(&win) = self.pending_expose.iter().next() {
            self.pending_expose.remove(&win);
            if let Some(state) = self.find_window_by_id(win) {
                if let Err(err) = self.redraw_titlebar(state) {
                    eprintln!(
                        "OWM_ERROR: while redrawing window {:x?}: {:?}.",
                        state.window, err
                    );
                }
            }
        }
    }

    fn grab_keycode(
        &self,
        window: Window,
        modifiers: KeyButMask,
        keycode: u8,
    ) -> Result<(), ReplyError> {
        self.conn.grab_key(
            true,
            window,
            modifiers,
            keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )?;

        Ok(())
    }

    fn find_window_by_id(&self, win: Window) -> Option<&WindowState> {
        self.windows
            .iter()
            .find(|state| state.window == win || state.frame_window == win)
    }

    fn find_window_by_id_mut(&mut self, win: Window) -> Option<&mut WindowState> {
        self.windows
            .iter_mut()
            .find(|state| state.window == win || state.frame_window == win)
    }

    fn handle_event(&mut self, event: Event) -> Result<(), ReplyOrIdError> {
        let mut should_ignore = false;
        if let Some(seqno) = event.wire_sequence_number() {
            while let Some(&Reverse(to_ignore)) = self.sequences_to_ignore.peek() {
                if to_ignore.wrapping_sub(seqno) <= u16::max_value() / 2 {
                    should_ignore = to_ignore == seqno;
                    break;
                }
                self.sequences_to_ignore.pop();
            }
        }

        if should_ignore {
            println!("[ignored]");
            return Ok(());
        }

        match event {
            Event::UnmapNotify(event) => self.handle_unmap_notify(event),
            Event::ConfigureRequest(event) => self.handle_configure_request(event)?,
            Event::MapRequest(event) => self.handle_map_request(event)?,
            Event::Expose(event) => self.handle_expose(event),
            Event::ButtonPress(event) => self.handle_button_press(event)?,
            Event::ButtonRelease(event) => self.handle_button_release(event)?,
            Event::MotionNotify(event) => self.handle_motion_notify(event)?,
            Event::KeyRelease(event) => self.handle_key_release(event)?,
            _ => {}
        }
        Ok(())
    }

    fn handle_unmap_notify(&mut self, event: UnmapNotifyEvent) {
        let root = self.conn.setup().roots[self.screen_num].root;
        let conn = self.conn;
        self.windows.retain(|state| {
            if state.window != event.window {
                return true;
            }
            conn.change_save_set(SetMode::DELETE, state.window).unwrap();
            conn.reparent_window(state.window, root, state.x, state.y)
                .unwrap();
            conn.destroy_window(state.frame_window).unwrap();
            false
        });
    }

    fn handle_configure_request(&mut self, event: ConfigureRequestEvent) -> Result<(), ReplyError> {
        if let Some(state) = self.find_window_by_id_mut(event.window) {
            let _ = state;
            unimplemented!();
        }

        let aux = ConfigureWindowAux::from_configure_request(&event)
            .sibling(None)
            .stack_mode(None);

        self.conn.configure_window(event.window, &aux)?;

        Ok(())
    }

    fn handle_map_request(&mut self, event: MapRequestEvent) -> Result<(), ReplyOrIdError> {
        self.manage_window(
            event.window,
            &self.conn.get_geometry(event.window)?.reply()?,
        )
    }

    fn handle_expose(&mut self, event: ExposeEvent) {
        self.pending_expose.insert(event.window);
    }

    fn handle_key_release(&self, event: KeyReleaseEvent) -> Result<(), ReplyError> {
        if event.detail == 24 {
            if let Some(state) = self.find_window_by_id(event.child) {
                let event = ClientMessageEvent::new(
                    32,
                    state.window,
                    self.wm_protocols,
                    [self.wm_delete_window, 0, 0, 0, 0],
                );
                self.conn
                    .send_event(false, state.window, EventMask::NO_EVENT, &event)?;
            }
        }

        Ok(())
    }

    fn handle_button_press(&mut self, event: ButtonPressEvent) -> Result<(), ReplyError> {
        if event.detail != DRAG_BUTTON || event.state != 0 {
            return Ok(());
        }

        if let Some(state) = self.find_window_by_id(event.event) {
            self.conn
                .set_input_focus(InputFocus::PARENT, state.window, CURRENT_TIME)?;
            self.conn.configure_window(
                state.frame_window,
                &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            )?;

            if self.drag_window.is_none() && event.event_x < state.close_area() {
                let (x, y) = (-event.event_x, -event.event_y);
                self.drag_window = Some((state.frame_window, (x, y)));
            }
        }

        Ok(())
    }

    fn handle_button_release(&mut self, event: ButtonReleaseEvent) -> Result<(), ReplyError> {
        if event.detail == DRAG_BUTTON {
            self.drag_window = None;
        }

        if let Some(state) = self.find_window_by_id(event.event) {
            if event.event_x >= state.close_area() {
              let event = ClientMessageEvent::new(
                    32,
                    state.window,
                    self.wm_protocols,
                    [self.wm_delete_window, 0, 0, 0, 0],
                );
                self.conn
                    .send_event(false, state.window, EventMask::NO_EVENT, &event)?;
            }
        }

        Ok(())
    }

    fn handle_motion_notify(&mut self, event: MotionNotifyEvent) -> Result<(), ReplyError> {
        if let Some((win, (x, y))) = self.drag_window {
            let (x, y) = (x + event.root_x, y + event.root_y);
            let (x, y) = (x as i32, y as i32);
            self.conn
                .configure_window(win, &ConfigureWindowAux::new().x(x).y(y))?;
        }

        Ok(())
    }
}

fn change<C: Connection>(conn: &C, screen: &Screen) -> Result<(), ReplyError> {
    let change = ChangeWindowAttributesAux::default()
        .event_mask(EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY);
    let res = conn.change_window_attributes(screen.root, &change)?.check();
    if let Err(ReplyError::X11Error(ref error)) = res {
        if error.error_kind == ErrorKind::Access {
            eprintln!("OWM_MESSAGE: Another WM is already running.");
            exit(1);
        } else {
            res
        }
    } else {
        res
    }
}

fn main() {
    let (connection, screen_num) = x11rb::connect(None).unwrap();

    let screen = &connection.setup().roots[screen_num];

    change(&connection, screen).unwrap();

    let mut wm_state = WmState::new(&connection, screen_num).unwrap();
    wm_state.scan_windows().unwrap();

    wm_state
        .grab_keycode(screen.root, KeyButMask::MOD1, 24)
        .unwrap();
    wm_state
        .grab_keycode(screen.root, KeyButMask::LOCK | KeyButMask::MOD1, 24)
        .unwrap();
    wm_state
        .grab_keycode(
            screen.root,
            KeyButMask::MOD2 | KeyButMask::LOCK | KeyButMask::MOD1,
            24,
        )
        .unwrap();

    loop {
        wm_state.refresh();
        connection.flush().unwrap();

        let event = connection.wait_for_event().unwrap();
        let mut event_option = Some(event);
        while let Some(event) = event_option {
            wm_state.handle_event(event).unwrap();
            event_option = connection.poll_for_event().unwrap();
        }
    }
}

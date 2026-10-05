//! The floor's drawing (the cubes, the words, the boards, the buttons) as a
//! widget of its own: what is drawn recorded first, and only the part with
//! something in it handed to GTK to set in pixels. A DrawingArea's picture
//! is the whole widget's, cleared and uploaded each frame however little is
//! on it: 5 ms a frame in the window, 15-20 on the whole monitor.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{cairo, graphene};

type Draw = Box<dyn Fn(&cairo::Context, i32, i32)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct FloorArea {
        pub draw: RefCell<Option<Draw>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FloorArea {
        const NAME: &'static str = "ItemgridFloorArea";
        type Type = super::FloorArea;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for FloorArea {}

    impl WidgetImpl for FloorArea {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let draw = self.draw.borrow();
            let Some(draw) = draw.as_ref() else { return };
            let (w, h) = (self.obj().width(), self.obj().height());
            let Ok(rec) = cairo::RecordingSurface::create(cairo::Content::ColorAlpha, None) else { return };
            if let Ok(cr) = cairo::Context::new(&rec) {
                draw(&cr, w, h);
            }
            // What was drawn, in whole pixels, within the widget.
            let (x, y, ew, eh) = rec.ink_extents();
            let (x0, y0) = (x.floor().max(0.0), y.floor().max(0.0));
            let (x1, y1) = ((x + ew).ceil().min(w as f64), (y + eh).ceil().min(h as f64));
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            let cr = snapshot.append_cairo(&graphene::Rect::new(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32));
            if cr.set_source_surface(&rec, 0.0, 0.0).is_ok() {
                let _ = cr.paint();
            }
        }
    }
}

glib::wrapper! {
    pub struct FloorArea(ObjectSubclass<imp::FloorArea>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FloorArea {
    pub fn new() -> FloorArea {
        let area: FloorArea = glib::Object::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_can_target(false);
        area
    }

    /// What it draws (a cairo context, its width and height).
    pub fn set_draw_func(&self, draw: impl Fn(&cairo::Context, i32, i32) + 'static) {
        *self.imp().draw.borrow_mut() = Some(Box::new(draw));
        self.queue_draw();
    }
}

//! The Duo's parts (its halves' pictures, the cord, the 3D room, the
//! hinge) held and turned in 3D: each child allocated once at its own size
//! and set with a transform of its own. A gtk::Fixed did this, but a
//! transform set on it queues a resize, and a resize goes up to the window:
//! the whole window laid out again each frame the phone moved (1-3 ms a
//! frame, 7-9 when it caught something bigger - the frames the camera
//! dropped). Here a transform changed only allocates the children again
//! (queue_allocate: this widget alone), and the order the parts are drawn
//! in is this widget's own list, not the tree's (moving children in the
//! tree each frame invalidated their style too).

use std::cell::RefCell;

use gtk::glib;
use gtk::gsk;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    /// The children in the order they are drawn (the last on top), each
    /// with its transform.
    #[derive(Default)]
    pub struct Stage {
        pub children: RefCell<Vec<(gtk::Widget, Option<gsk::Transform>)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Stage {
        const NAME: &'static str = "ItemgridStage";
        type Type = super::Stage;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Stage {
        fn dispose(&self) {
            for (child, _) in self.children.borrow_mut().drain(..) {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Stage {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::ConstantSize
        }

        /// No size of its own: laid over the room (an overlay's child,
        /// filling it); its parts reach past it as they will.
        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }

        fn size_allocate(&self, _width: i32, _height: i32, _baseline: i32) {
            for (child, transform) in self.children.borrow().iter() {
                if !child.should_layout() {
                    continue;
                }
                // (At its least, as gtk::Fixed had it: a picture asked for
                // its height at a width grows to its drawing's shape.)
                let (w, _, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
                let (h, _, _, _) = child.measure(gtk::Orientation::Vertical, -1);
                child.allocate(w, h, -1, transform.clone());
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            for (child, _) in self.children.borrow().iter() {
                if child.should_layout() {
                    obj.snapshot_child(child, snapshot);
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct Stage(ObjectSubclass<imp::Stage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Stage {
    fn default() -> Stage {
        Stage::new()
    }
}

impl Stage {
    pub fn new() -> Stage {
        glib::Object::new()
    }

    fn index(&self, child: &gtk::Widget) -> Option<usize> {
        self.imp().children.borrow().iter().position(|(c, _)| c == child)
    }

    /// A part added, drawn over those before it, at (x, y).
    pub fn put(&self, child: &impl IsA<gtk::Widget>, x: f64, y: f64) {
        let child = child.as_ref().clone();
        child.set_parent(self);
        let at = gsk::Transform::new().translate(&gtk::graphene::Point::new(x as f32, y as f32));
        self.imp().children.borrow_mut().push((child, Some(at)));
        self.queue_allocate();
    }

    /// A part's transform (none: at the origin), its allocation alone
    /// redone when it changed.
    pub fn set_child_transform(&self, child: &impl IsA<gtk::Widget>, transform: Option<&gsk::Transform>) {
        let Some(i) = self.index(child.as_ref()) else { return };
        let mut children = self.imp().children.borrow_mut();
        let transform = transform.cloned();
        if children[i].1 != transform {
            children[i].1 = transform;
            drop(children);
            self.queue_allocate();
        }
    }

    /// The part drawn first (under the others).
    pub fn first(&self) -> Option<gtk::Widget> {
        self.imp().children.borrow().first().map(|(c, _)| c.clone())
    }

    /// `child` drawn just under `sibling` (none: over all the others).
    pub fn before(&self, child: &impl IsA<gtk::Widget>, sibling: Option<&gtk::Widget>) {
        let (Some(i), to) = (self.index(child.as_ref()), sibling.and_then(|s| self.index(s))) else { return };
        let mut children = self.imp().children.borrow_mut();
        let to = to.unwrap_or(children.len());
        if to == i + 1 || to == i {
            return;
        }
        let entry = children.remove(i);
        let to = if to > i { to - 1 } else { to };
        children.insert(to, entry);
        drop(children);
        self.queue_draw();
    }

    /// `child` drawn just over `sibling` (none: under all the others).
    pub fn after(&self, child: &impl IsA<gtk::Widget>, sibling: Option<&gtk::Widget>) {
        let (Some(i), to) = (self.index(child.as_ref()), sibling.and_then(|s| self.index(s))) else { return };
        let mut children = self.imp().children.borrow_mut();
        let to = to.map_or(0, |t| t + 1);
        if to == i + 1 || to == i {
            return;
        }
        let entry = children.remove(i);
        let to = if to > i { to - 1 } else { to };
        children.insert(to, entry);
        drop(children);
        self.queue_draw();
    }
}

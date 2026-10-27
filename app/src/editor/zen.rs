//! Zen (shell-design §5.3): [`ZenBin`], a `gtk::Widget` subclass giving its child 2/3 of the width
//! and 92 % of the height, centred — the Mac's fractions, DPI-free — while Zen is on, and all of
//! itself otherwise; and [`ZenCapsule`], the write / read / exit switch in the window overlay that
//! fades after 2.5 s (md-logic's `ZenController` owns the timer and the state).

use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use md_logic::strings::zen as strings;
use md_logic::view::ZenController;

/// The Mac's Zen column: two thirds of the window's width.
pub const WIDTH_FRACTION: f64 = 2.0 / 3.0;
/// … and 92 % of its height.
pub const HEIGHT_FRACTION: f64 = 0.92;

/// The capsule's fade (shell-design §5.3).
const CAPSULE_FADE_MS: u32 = 300;

/// Where a child of `min` minimum size goes in a bin of `size` along one axis: the fraction of the
/// bin, never less than the child's minimum, never more than the bin; `(offset, length)` centred.
pub fn centred(size: i32, fraction: f64, min: i32) -> (i32, i32) {
    let size = size.max(0);
    let length = ((f64::from(size) * fraction).round() as i32).max(min).min(size);
    ((size - length) / 2, length)
}

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Default)]
    pub struct ZenBin {
        pub child: RefCell<Option<gtk::Widget>>,
        pub active: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ZenBin {
        const NAME: &'static str = "MdZenBin";
        type Type = super::ZenBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ZenBin {
        fn dispose(&self) {
            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ZenBin {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            match self.child.borrow().as_ref() {
                Some(child) => child.request_mode(),
                None => gtk::SizeRequestMode::ConstantSize,
            }
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match self.child.borrow().as_ref() {
                Some(child) if child.should_layout() => {
                    let (min, natural, _, _) = child.measure(orientation, for_size);
                    (min, natural, -1, -1)
                }
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let child = self.child.borrow().clone();
            let Some(child) = child else { return };
            if !child.should_layout() {
                return;
            }
            if !self.active.get() {
                child.allocate(width, height, baseline, None);
                return;
            }
            let (min_width, _, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            let (x, w) = centred(width, WIDTH_FRACTION, min_width);
            let (min_height, _, _, _) = child.measure(gtk::Orientation::Vertical, w);
            let (y, h) = centred(height, HEIGHT_FRACTION, min_height);
            let at = gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x as f32, y as f32));
            child.allocate(w, h, -1, Some(at));
        }
    }
}

glib::wrapper! {
    /// The single-child bin that lays out the Zen column (see the module comment).
    pub struct ZenBin(ObjectSubclass<imp::ZenBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ZenBin {
    pub fn new(child: &impl IsA<gtk::Widget>) -> ZenBin {
        let bin: ZenBin = glib::Object::builder().property("hexpand", true).property("vexpand", true).build();
        child.set_parent(&bin);
        *bin.imp().child.borrow_mut() = Some(child.clone().upcast());
        bin
    }

    /// Zen on: the child gets the centred column; off: all of the bin.
    pub fn set_active(&self, active: bool) {
        if self.imp().active.replace(active) != active {
            self.queue_allocate();
        }
    }

    pub fn is_active(&self) -> bool {
        self.imp().active.get()
    }

    pub fn child(&self) -> Option<gtk::Widget> {
        self.imp().child.borrow().clone()
    }
}

/// The capsule of one document window: write, read, a divider, exit — in the window's overlay,
/// top centre, faded by md-logic's 2.5 s timer and revealed by pointer movement.
pub struct ZenCapsule {
    me: Weak<ZenCapsule>,
    zen: Rc<ZenController>,
    revealer: gtk::Revealer,
    write: gtk::Button,
    read: gtk::Button,
    exit: gtk::Button,
}

impl ZenCapsule {
    pub fn new(overlay: &gtk::Overlay, zen: Rc<ZenController>) -> Rc<ZenCapsule> {
        let button = |icon: &str, tooltip: &str| {
            let b = gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).build();
            b.add_css_class("flat");
            b
        };
        let write = button("document-edit-symbolic", strings::WRITE);
        let read = button("view-reveal-symbolic", strings::READ);
        let exit = button("view-restore-symbolic", strings::EXIT);
        let capsule = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        capsule.add_css_class("md-zen-capsule");
        capsule.append(&write);
        capsule.append(&read);
        capsule.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        capsule.append(&exit);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(CAPSULE_FADE_MS)
            .child(&capsule)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(10)
            .reveal_child(false)
            .visible(false)
            .build();
        overlay.add_overlay(&revealer);

        let this = Rc::new_cyclic(|me| ZenCapsule { me: me.clone(), zen, revealer, write, read, exit });
        this.wire(overlay);
        this.refresh();
        this
    }

    /// Re-reads Zen's state: shown only in Zen, the active switch tinted, faded while the
    /// controller says the controls are hidden. The window calls it on every
    /// `DocumentWindowState::changed` (the switch can also move by Ctrl+1 / Ctrl+3).
    pub fn refresh(&self) {
        let active = self.zen.is_active();
        let reading = self.zen.is_reading();
        self.revealer.set_visible(active);
        self.revealer.set_reveal_child(active && self.zen.controls_shown());
        self.revealer.set_can_target(active && self.zen.controls_shown());
        for (button, on) in [(&self.write, !reading), (&self.read, reading)] {
            if on {
                button.add_css_class("md-zen-active");
            } else {
                button.remove_css_class("md-zen-active");
            }
        }
    }

    /// Whether the capsule is up (shown, not faded).
    pub fn is_shown(&self) -> bool {
        self.revealer.is_visible() && self.revealer.reveals_child()
    }

    /// The capsule's revealer, for the self-test's geometry.
    pub fn widget(&self) -> gtk::Widget {
        self.revealer.clone().upcast()
    }

    /// The three switches as a pointer would press them: 0 write, 1 read, 2 exit.
    pub fn press(&self, which: usize) {
        match which {
            0 => self.write.emit_clicked(),
            1 => self.read.emit_clicked(),
            _ => self.exit.emit_clicked(),
        }
    }

    fn wire(&self, overlay: &gtk::Overlay) {
        let me = self.me.clone();
        let with = move |f: &dyn Fn(&ZenCapsule)| {
            if let Some(capsule) = me.upgrade() {
                f(&capsule);
            }
        };
        let w = with.clone();
        self.write.connect_clicked(move |_| {
            w(&|c| {
                c.zen.set_reading(false);
                c.refresh();
            })
        });
        let w = with.clone();
        self.read.connect_clicked(move |_| {
            w(&|c| {
                c.zen.set_reading(true);
                c.refresh();
            })
        });
        let w = with.clone();
        self.exit.connect_clicked(move |_| {
            w(&|c| {
                c.zen.set_active(false);
                c.refresh();
            })
        });
        let w = with.clone();
        self.zen.controls_shown_changed().connect(move |_| w(&|c| c.refresh()));
        let w = with.clone();
        self.zen.full_screen_requested().connect(move |_| w(&|c| c.refresh()));

        let motion = gtk::EventControllerMotion::new();
        let w = with;
        motion.connect_motion(move |_, _, _| w(&|c| c.zen.reveal()));
        overlay.add_controller(motion);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_column_is_two_thirds_by_ninety_two_percent_centred() {
        assert_eq!(centred(1440, WIDTH_FRACTION, 0), (240, 960));
        assert_eq!(centred(900, HEIGHT_FRACTION, 0), (36, 828));
        // An odd remainder rounds the offset down.
        assert_eq!(centred(1001, WIDTH_FRACTION, 0), (167, 667));
    }

    #[test]
    fn the_child_minimum_wins_and_the_bin_caps_it() {
        assert_eq!(centred(300, WIDTH_FRACTION, 250), (25, 250));
        assert_eq!(centred(300, WIDTH_FRACTION, 400), (0, 300));
        assert_eq!(centred(0, WIDTH_FRACTION, 10), (0, 0));
        assert_eq!(centred(-5, WIDTH_FRACTION, 0), (0, 0));
    }
}

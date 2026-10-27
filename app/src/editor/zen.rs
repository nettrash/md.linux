//! Zen (shell-design §5.3): `ZenBin`, a `gtk::Widget` subclass giving its child 2/3 of the width and
//! 92 % of the height, centred; and `ZenCapsule`, the write / read / exit switch in the window
//! overlay that fades after 2.5 s.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P3

use std::rc::Rc;

use md_logic::view::ZenController;

/// The capsule of one document window.
pub struct ZenCapsule {
    zen: Rc<ZenController>,
}

impl ZenCapsule {
    pub fn new(overlay: &gtk::Overlay, zen: Rc<ZenController>) -> Rc<ZenCapsule> {
        Rc::new(ZenCapsule { zen })
    }
}

use std::collections::HashMap;

use zpadder_model::{KeyCode, MouseButton, OutputAction};

/// An output that stays down until released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Held {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl Held {
    pub(crate) fn is_modifier(self) -> bool {
        matches!(self, Held::Key(key) if key.is_modifier())
    }
}

/// Reference counts for held outputs across every control and pad: an output goes down when its
/// first holder presses it and up when its last holder lets go.
#[derive(Debug, Default)]
pub(crate) struct HeldOutputs {
    counts: HashMap<Held, u32>,
}

impl HeldOutputs {
    /// Adds a holder to `held`. `after_modifier` says that the same group of outputs has just
    /// pressed a modifier, as in Ctrl+Z.
    ///
    /// A key that another control already holds does not go down again, so an application
    /// would see Ctrl go down but never Z. In a combination, such a key is therefore tapped up
    /// and down again; it stays held throughout as far as its other holders are concerned.
    /// Mouse buttons are left alone, so that a combination cannot break another control's drag.
    pub(crate) fn press(&mut self, held: Held, after_modifier: bool, out: &mut Vec<OutputAction>) {
        let count = self.counts.entry(held).or_insert(0);
        *count += 1;
        if *count == 1 {
            out.push(action(held, true));
        } else if after_modifier && matches!(held, Held::Key(key) if !key.is_modifier()) {
            out.push(action(held, false));
            out.push(action(held, true));
        }
    }

    pub(crate) fn release(&mut self, held: Held, out: &mut Vec<OutputAction>) {
        let Some(count) = self.counts.get_mut(&held) else { return };
        *count -= 1;
        if *count == 0 {
            self.counts.remove(&held);
            out.push(action(held, false));
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}

fn action(held: Held, pressed: bool) -> OutputAction {
    match (held, pressed) {
        (Held::Key(key), true) => OutputAction::KeyDown(key),
        (Held::Key(key), false) => OutputAction::KeyUp(key),
        (Held::Mouse(button), pressed) => OutputAction::MouseButton { button, pressed },
    }
}

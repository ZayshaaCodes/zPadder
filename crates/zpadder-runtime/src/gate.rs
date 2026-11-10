//! Holds mouse output back while the test bench is in use, so a pad can be tried inside ZPadder
//! without clicking around the editor.

use zpadder_model::{MouseButton, OutputAction};

/// Decides which outputs reach Windows.
///
/// While `active` (ZPadder has focus and the test bench keeps mapping on), mouse buttons, pointer
/// movement and the wheel are held back; keys still go out. The gate remembers which mouse
/// buttons it let down, so the release of a button that was sent always goes out too, whenever it
/// comes, and the release of a button whose press was held back is held back as well. A mouse
/// button can therefore neither stick nor be released behind the user's back when the mode flips.
#[derive(Debug, Default)]
pub(crate) struct MouseGate {
    pub(crate) active: bool,
    /// Mouse buttons sent down to Windows and not yet released.
    down: Vec<MouseButton>,
}

impl MouseGate {
    /// Splits `actions`, in order, into those to send and those held back.
    pub(crate) fn filter(
        &mut self,
        actions: &[OutputAction],
        sent: &mut Vec<OutputAction>,
        held_back: &mut Vec<OutputAction>,
    ) {
        for &action in actions {
            let send = match action {
                OutputAction::KeyDown(_) | OutputAction::KeyUp(_) => true,
                OutputAction::MouseButton { button, pressed: true } => {
                    if !self.active && !self.down.contains(&button) {
                        self.down.push(button);
                    }
                    !self.active
                }
                OutputAction::MouseButton { button, pressed: false } => {
                    match self.down.iter().position(|&b| b == button) {
                        Some(i) => {
                            self.down.swap_remove(i);
                            true
                        }
                        None => false,
                    }
                }
                OutputAction::MouseMove { .. } | OutputAction::Wheel { .. } => !self.active,
            };
            if send {
                sent.push(action);
            } else {
                held_back.push(action);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use zpadder_model::KeyCode;

    use super::*;

    fn mouse(button: MouseButton, pressed: bool) -> OutputAction {
        OutputAction::MouseButton { button, pressed }
    }

    fn run(
        gate: &mut MouseGate,
        actions: &[OutputAction],
    ) -> (Vec<OutputAction>, Vec<OutputAction>) {
        let (mut sent, mut held_back) = (Vec::new(), Vec::new());
        gate.filter(actions, &mut sent, &mut held_back);
        (sent, held_back)
    }

    #[test]
    fn everything_goes_out_when_inactive() {
        let mut gate = MouseGate::default();
        let actions = [
            OutputAction::KeyDown(KeyCode::A),
            mouse(MouseButton::Left, true),
            OutputAction::MouseMove { dx: 3, dy: -1 },
            OutputAction::Wheel { vertical: 120, horizontal: 0 },
            mouse(MouseButton::Left, false),
            OutputAction::KeyUp(KeyCode::A),
        ];
        assert_eq!(run(&mut gate, &actions), (actions.to_vec(), Vec::new()));
    }

    #[test]
    fn active_gate_sends_keys_and_holds_mouse_back() {
        let mut gate = MouseGate { active: true, ..MouseGate::default() };
        let (sent, held_back) = run(
            &mut gate,
            &[
                OutputAction::KeyDown(KeyCode::A),
                mouse(MouseButton::Right, true),
                OutputAction::MouseMove { dx: 3, dy: -1 },
                OutputAction::Wheel { vertical: -120, horizontal: 0 },
                mouse(MouseButton::Right, false),
            ],
        );
        assert_eq!(sent, [OutputAction::KeyDown(KeyCode::A)]);
        assert_eq!(held_back.len(), 4);
    }

    #[test]
    fn a_button_sent_down_is_released_even_while_testing() {
        let mut gate = MouseGate::default();
        assert_eq!(run(&mut gate, &[mouse(MouseButton::Left, true)]).0.len(), 1);
        gate.active = true;
        let (sent, held_back) = run(&mut gate, &[mouse(MouseButton::Left, false)]);
        assert_eq!((sent, held_back), (vec![mouse(MouseButton::Left, false)], Vec::new()));
    }

    #[test]
    fn a_held_back_press_never_gets_a_stray_release() {
        let mut gate = MouseGate { active: true, ..MouseGate::default() };
        run(&mut gate, &[mouse(MouseButton::Left, true)]);
        gate.active = false;
        let (sent, held_back) = run(&mut gate, &[mouse(MouseButton::Left, false)]);
        assert_eq!((sent, held_back), (Vec::new(), vec![mouse(MouseButton::Left, false)]));
        // The next press and release go out normally.
        let actions = [mouse(MouseButton::Left, true), mouse(MouseButton::Left, false)];
        assert_eq!(run(&mut gate, &actions).0, actions);
    }
}

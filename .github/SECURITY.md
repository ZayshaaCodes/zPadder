# Security policy

ZPadder sends keyboard and mouse input on your behalf, so security bugs matter. Thank you for
reporting them responsibly.

## Reporting a vulnerability

Please **don't open a public issue**. Report it privately through GitHub instead: open the
repository's **Security** tab and choose **Report a vulnerability**. Include:

- the ZPadder version (Help menu, or the zip's name) and your Windows version;
- what an attacker can do, and what they need first (a file you open, local access, another
  user account…);
- steps or a file that reproduce it.

You'll get a reply within a week. Once a fix is released, we'll credit you in the changelog
unless you'd rather not be named.

## Supported versions

Only the latest release gets security fixes.

## What's in scope

Some examples:

- profile, layout or Xpadder files (`.toml`, `.xpadderprofile`, `.xpaddercontroller`) that
  crash ZPadder, hang it, or make it do something the file shouldn't be able to;
- the single-instance pipe that later launches use to pass their command line to the running
  ZPadder: other users or less-trusted processes sending it commands;
- DLL loading, or anything else that lets a file next to `zpadder.exe` run code;
- input sent somewhere it shouldn't go, such as keys left held down.

Games blocking or flagging ZPadder's input is not a vulnerability; see
[docs/anti-cheat.md](../docs/anti-cheat.md).

## What ZPadder does and doesn't do

- It makes no network connections and has no telemetry.
- It reads the name and title of the window in front, for AutoProfile, and the names of running
  programs, to warn about other controller software. It keeps neither beyond the rules you set.
- It stores settings, imported controller pictures and its log in `%APPDATA%\ZPadder`, or next
  to `zpadder.exe` in portable mode.

# cradle

Look after a connected Surface Duo from a Linux computer: see what it is doing, update item, read its logs, take screenshots, and later back it up and flash it - safely.

First for the project's own daily work, then for people who run item on their Duo.

## How it is built

- **cradle-core** (`crates/core`): a Rust library. It finds out what the phone is doing, carries out the actions, and holds the safety rules. Every action and every rule lives here, so no interface can go around them.
- **cradle** (`crates/cli`): the command line over the core, e.g. `cradle status`, `cradle update`, `cradle logs --boot -1`.
- **The window** (later): GTK4/libadwaita over the same core.

## What the phone is doing

| Mode | Seen as | What can be done |
|---|---|---|
| Linux (Droidian + item) | the USB network, ssh to 172.16.42.1 | status, update, logs, shell, screenshots, reboot |
| fastboot | `fastboot devices` | boot an image from RAM; flash only after a good RAM boot |
| Recovery (TWRP) | adb | backups, restores |
| Android (a test image) | adb | recognised only, at first; switching images comes with flashing |
| Not seen | - | hints: the cable, the battery, the keys |

## Stages

1. **Daily work**
   - `status`: the mode; on Linux the versions of item and the port, the battery, heat, free space, uptime, failed services.
   - `update`: build item, install it, reboot, wait for the boot and say "enter the PIN".
   - `logs`: the journal of this boot or an earlier one, filtered (item, sensorfw, the kernel), saved to a file for a ticket.
   - `shell` / `run`: a shell on the phone, or one command, sent as a script, with the dangerous ones refused.
   - `reboot`: and wait until the phone is back.
   - `screenshot`: both panels as one picture on the computer.
2. **Flashing and restoring**
   - Backups of partitions and the rootfs.
   - Booting an image from RAM through fastboot.
   - Flashing only after a successful RAM boot, one change per boot, asked for explicitly.
   - Going back to a backup.
   - Switching between the phone's own system and test images (Lindroid and others).
3. **The window**
   - The status screen and updates first.
   - Then flashing, step by step.

## Safety rules, as code

These are rules learned on the device. Here they are checks in the core, not things to remember.

- **Flashing:** RAM boot first and always; one change per boot; a backup before any write; never in one click.
- **Never read `/sys/kernel/debug/gpio` on the Duo:** it resets the phone at once. Such commands are refused.
- **Updates go through a reboot:** restarting the vendor hwcomposer resets the phone about one time in three (item-tracker #119).
- **Commands are sent as a script file.** A `pkill -f` pattern sent inline over ssh matches its own shell and kills it.
- **The owner's settings, keys and system are left as they were.**

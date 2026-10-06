# The club's numbers (registry)

Every Duo item/grid sees gets a number for good: 00001, 00002, ... in the
order they first turn up. Quietly - no account, no token, no site to visit.
The number belongs to the phone (its serial), not to a person: it stays
through a return to Android, a reinstall, another computer, a new owner.

## The app's side (crates/core/src/club.rs)

- When item/grid first sees a phone's serial - on Linux, in stock Android
  with USB debugging, in the bootloader, in the recovery - and has no number
  for it, it asks the club once a run (`claim`). Off the network, nothing is
  said; it asks again next run.
- Sent: `{"serial_hash": sha256("cradle-device:" + serial), "model": "surfaceduo"}`.
  The serial itself never leaves the computer. (The prefix is kept from
  Cradle: 00001 is registered under it.)
- Kept here: `~/.local/share/itemgrid/devices.json` (club + serial ->
  number). Written onto the phone: `/etc/item/device-id`.
- A club without the endpoint yet (404/405): the old
  `POST /api/registry/devices` with a token kept in the keyring from before,
  if there is one.

## The server's side (to build on agentsco.uk)

`POST /api/registry/claim`, no authentication, JSON body as above.

- The hash already known: `200` with its device -
  `{"number": "00001", "model": "surfaceduo", "label": null, "new": false}`.
- Not known: the next number (a sequence; numbers are never reused, a gap
  is fine), stored, `201` with `"new": true`.
- Stored keyed: the server keeps HMAC(server secret, serial_hash), not the
  hash itself - a serial is about 12 digits, so a plain hash could be
  brute-forced back from a leaked table.
- Limited: new numbers per client address (say 3 a day); over it, `429`.
  Known hashes are always answered.
- `400` for a body that is not a 64-hex `serial_hash` or another `model`.
- The existing devices (00001, from the token way) answer through it as is:
  same table, same keyed hash.

Later (not now): labels, the phone's item/port versions, an owner who
wants to claim their number on the site.

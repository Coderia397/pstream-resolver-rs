# Deploying to the phone

The resolver runs on an Android phone under Termux. It is a single static
aarch64 binary — no Node, no `node_modules`, nothing to install at deploy time.

## Once, per device

**1. Termux and Termux:Boot** — from F-Droid, not Play Store. The Play version
is stale and its boot add-on doesn't work.

**2. Packages**

```sh
pkg install cloudflared termux-api
termux-wake-lock
```

**3. Battery** — Settings → Apps → Termux → Battery → **Unrestricted**, for
`com.termux`, `com.termux.api` and `com.termux.boot`. Without it Android kills
the resolver whenever the screen has been off a while.

**4. Phantom process killer** — Android 12+ kills apps that spawn many child
processes, which is exactly what three supervisor loops look like. From a
computer with the phone on USB:

```sh
adb shell "/system/bin/device_config put activity_manager max_phantom_processes 2147483647"
adb shell "settings put global settings_enable_monitor_phantom_procs false"
```

`device_config` values can reset on reboot; the `settings` one persists.

**5. Cloudflare tunnel**

```sh
cloudflared tunnel login
cloudflared tunnel create pstream-resolver
cloudflared tunnel route dns pstream-resolver resolver.example.com
```

**6. Secrets**

```sh
cp pstream-env.example ~/.pstream-env
chmod 600 ~/.pstream-env
# then edit it
```

**7. Boot script**

```sh
mkdir -p ~/.termux/boot
cp start-pstream.sh ~/.termux/boot/start-pstream.sh
chmod 700 ~/.termux/boot/start-pstream.sh
```

**8. First binary** — the updater only replaces an existing file, so seed it:

```sh
curl -fsSL -o ~/pstream-resolver-rs \
  https://github.com/Coderia397/pstream-resolver-rs/releases/latest/download/pstream-resolver-aarch64
chmod 755 ~/pstream-resolver-rs
sh ~/.termux/boot/start-pstream.sh
```

## After that

Nothing. Push to `main`, CI publishes a new binary, the device swaps it in
within five minutes. No cable.

## Checking on it

```sh
tail -f ~/resolver.log     # the resolver
tail -f ~/tunnel.log       # cloudflared
tail -f ~/deploy.log       # updater: what it swapped and when
```

Three supervisors should be running, and both processes should have a parent
that is **not** PID 1 — a PPid of 1 means the supervisor died and nothing will
restart that process:

```sh
ps -A -o PID,PPID,NAME | grep -E 'pstream-resolve|cloudflared'
```

## When it breaks

**Cloudflare 1033 / tunnel won't register.** `cloudflared` defaults to QUIC on
UDP 7844, which many public networks drop. The script already forces
`--protocol http2` (TCP). If it still fails, the network is blocking port 7844
on both protocols and no flag will help — use a different connection. Mobile
data usually works where cafe and library WiFi doesn't.

**Deploys stop arriving.** Check `~/deploy.log`. A checksum mismatch means a
truncated download, which the updater discards on purpose rather than
installing a corrupt binary.

**The resolver is up but the phone shows nothing.** That's normal — it has no
UI. Check `curl localhost:8790/`.

**Replacing the binary by hand fails with "Text file busy".** You cannot write
over a running executable. Write beside it and rename:

```sh
cat new-binary > ~/.res.new && chmod 755 ~/.res.new && mv ~/.res.new ~/pstream-resolver-rs
pkill -x pstream-resolve
```

Note `pstream-resolve`, not the full name — Linux truncates process names to
15 characters, so `pkill -x pstream-resolver-rs` matches nothing.

#!/data/data/com.termux/files/usr/bin/sh
# Auto-start the resolver + named tunnel on phone boot (Termux:Boot).
#
# Install to ~/.termux/boot/start-pstream.sh with mode 700. See deploy/README.md.
#
# Three supervised loops, because nothing on Android restarts a crashed
# background process. Everything here once ran detached at PPid 1, so a single
# crash took the service down until the next reboot.
#
#   1. resolver   — the static Rust binary, restarted if it exits
#   2. cloudflared — the tunnel, likewise
#   3. updater    — pulls a new binary from GitHub Releases and swaps it in
#
# The updater replaced an earlier git-pull poller. That pulled the JS backend
# repo, which stopped being what runs once the resolver became a compiled
# binary — it was dutifully fetching source the device never executed.

export PREFIX=/data/data/com.termux/files/usr
export PATH=$PREFIX/bin:$PATH
export LD_LIBRARY_PATH=$PREFIX/lib
export HOME=/data/data/com.termux/files/home

# Secrets live outside the repo. Create ~/.pstream-env from
# deploy/pstream-env.example — without it the resolver still starts, and the
# features needing a key report themselves unconfigured.
[ -f "$HOME/.pstream-env" ] && . "$HOME/.pstream-env"

BIN=$HOME/pstream-resolver-rs
RELEASE=https://github.com/Coderia397/pstream-resolver-rs/releases/latest/download

termux-wake-lock

# ── 1. resolver ──────────────────────────────────────────────────────────────
# The 2s pause stops a crash-looping build from spinning the CPU.
setsid sh -c "
    while true; do
        PORT=8790 '$BIN' >> '$HOME/resolver.log' 2>&1
        echo \"[supervisor] resolver exited (\$?), restarting\" >> '$HOME/resolver.log'
        sleep 2
    done
" > /dev/null 2>&1 < /dev/null &

sleep 3

# ── 2. tunnel ────────────────────────────────────────────────────────────────
# --protocol http2 forces the tunnel over TCP rather than QUIC. Some networks
# drop UDP 7844, which shows up as Cloudflare error 1033 with cloudflared
# burning CPU in a retry loop. A network blocking port 7844 on both protocols
# cannot be worked around here — that needs a different connection.
setsid sh -c "
    while true; do
        cloudflared tunnel --protocol http2 run \
            --url http://localhost:8790 pstream-resolver \
            >> '$HOME/tunnel.log' 2>&1
        echo \"[supervisor] tunnel exited (\$?), restarting\" >> '$HOME/tunnel.log'
        sleep 5
    done
" > /dev/null 2>&1 < /dev/null &

# ── 3. updater ───────────────────────────────────────────────────────────────
# Compares the published checksum against what is installed and downloads only
# on a difference, so an idle phone costs one small request every 5 minutes.
#
# The swap is write-then-rename. Writing over the file directly fails with
# ETXTBSY while the binary is executing, and the supervisor restarts it within
# two seconds — rename() puts a new inode in place instead, which the running
# process is unaffected by, and the bounce afterwards picks it up.
#
# pkill -x matches the process name exactly, and Linux truncates that to 15
# characters in /proc/*/comm, so the name to match is "pstream-resolve", not
# the full filename. -f would also match these supervisor shells, whose command
# line contains this script's text.
setsid sh -c "
    while true; do
        sleep 300
        WANT=\$(curl -fsSL --max-time 30 '$RELEASE/pstream-resolver-aarch64.sha256' 2>/dev/null | tr -d '[:space:]')
        [ -n \"\$WANT\" ] || continue
        HAVE=\$(sha256sum '$BIN' 2>/dev/null | cut -d' ' -f1)
        [ \"\$WANT\" = \"\$HAVE\" ] && continue

        echo \"[updater] \$HAVE -> \$WANT\" >> '$HOME/deploy.log'
        if curl -fsSL --max-time 300 -o '$HOME/.res.new' '$RELEASE/pstream-resolver-aarch64' 2>/dev/null; then
            GOT=\$(sha256sum '$HOME/.res.new' 2>/dev/null | cut -d' ' -f1)
            if [ \"\$GOT\" = \"\$WANT\" ]; then
                chmod 755 '$HOME/.res.new'
                mv '$HOME/.res.new' '$BIN'
                pkill -x pstream-resolve
                echo '[updater] swapped and bounced' >> '$HOME/deploy.log'
            else
                # A truncated download must never reach the binary path.
                rm -f '$HOME/.res.new'
                echo '[updater] checksum mismatch, discarded' >> '$HOME/deploy.log'
            fi
        else
            echo '[updater] download failed' >> '$HOME/deploy.log'
        fi
    done
" > /dev/null 2>&1 < /dev/null &

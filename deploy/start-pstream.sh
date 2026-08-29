#!/data/data/com.termux/files/usr/bin/sh
# Auto-start the resolver + named tunnel on phone boot (Termux:Boot).
#
# Install to ~/.termux/boot/start-pstream.sh with mode 700. See deploy/README.md.
#
# Three supervised loops, because nothing on Android restarts a crashed
# background process. Everything here once ran detached at PPid 1, so a single
# crash took the service down until the next reboot.
#
#   1. resolver    — the static Rust binary, restarted if it exits
#   2. cloudflared — the tunnel, likewise
#   3. updater     — pulls a new binary from GitHub Releases and swaps it in
#
# Running this script twice starts a second set of everything. Three
# cloudflared instances fighting over one tunnel is not obvious from the
# outside, so the guard below refuses to start if a set is already running.

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
PORT=8790

# ── don't start a second copy ────────────────────────────────────────────────
# Use pidof, not pgrep. Termux ships procps-ng, whose pgrep matches the FULL
# process name, while /proc/*/comm truncates to 15 characters — so
# `pgrep -x pstream-resolve` silently matches nothing and the guard never
# fires. Verified on device:
#   pgrep -x pstream-resolve    -> (nothing)
#   pidof pstream-resolver-rs   -> the resolver pid, and only that
if pidof pstream-resolver-rs > /dev/null 2>&1; then
    echo "[boot] a resolver is already running; not starting a second set" >&2
    exit 0
fi

termux-wake-lock

# ── 1. resolver ──────────────────────────────────────────────────────────────
# The 2s pause stops a crash-looping build from spinning the CPU.
setsid sh -c "
    while true; do
        PORT=$PORT '$BIN' >> '$HOME/resolver.log' 2>&1
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
            --url http://localhost:$PORT pstream-resolver \
            >> '$HOME/tunnel.log' 2>&1
        echo \"[supervisor] tunnel exited (\$?), restarting\" >> '$HOME/tunnel.log'
        sleep 5
    done
" > /dev/null 2>&1 < /dev/null &

# ── 3. updater ───────────────────────────────────────────────────────────────
# Checks what the RUNNING PROCESS reports, not what is on disk.
#
# The previous version compared the on-disk checksum against the published
# one. That is satisfied the moment the file is written, so once it had
# swapped the file it could never again notice a problem. When a restart
# silently failed to take, the file was new, the process was old, and the
# updater stayed happy — it served a two-day-old build from a deleted inode
# while reporting itself up to date.
#
# Asking the running process for its version closes that hole: a swap that
# does not take is detected on the very next poll and retried.
setsid sh -c '
    BIN='"$BIN"'
    RELEASE='"$RELEASE"'
    PORT='"$PORT"'
    LOG='"$HOME"'/deploy.log

    while true; do
        sleep 300

        WANT=$(curl -fsSL --max-time 30 "$RELEASE/pstream-resolver-aarch64.sha256" 2>/dev/null | tr -d "[:space:]")
        [ -n "$WANT" ] || continue

        HAVE=$(sha256sum "$BIN" 2>/dev/null | cut -d" " -f1)

        # Download only when the published build differs from our copy.
        if [ "$WANT" != "$HAVE" ]; then
            echo "[updater] $(date +%FT%T) new build ${WANT%%??????????????????????????????????????????????}" >> "$LOG"
            if curl -fsSL --max-time 300 -o "$HOME/.res.new" "$RELEASE/pstream-resolver-aarch64" 2>/dev/null; then
                GOT=$(sha256sum "$HOME/.res.new" 2>/dev/null | cut -d" " -f1)
                if [ "$GOT" = "$WANT" ]; then
                    chmod 755 "$HOME/.res.new"
                    # Write-then-rename: overwriting a running executable fails
                    # with ETXTBSY, but rename() installs a new inode that the
                    # running process is unaffected by.
                    mv "$HOME/.res.new" "$BIN"
                    echo "[updater] $(date +%FT%T) installed" >> "$LOG"
                else
                    rm -f "$HOME/.res.new"
                    echo "[updater] $(date +%FT%T) checksum mismatch, discarded" >> "$LOG"
                    continue
                fi
            else
                echo "[updater] $(date +%FT%T) download failed" >> "$LOG"
                continue
            fi
        fi

        # Whether or not we just installed, make sure the RUNNING process is
        # on the installed binary. Comparing the executable the process holds
        # against the file on disk catches the case the old logic could not:
        # readlink reports "(deleted)" once the inode has been replaced.
        PID=$(pidof pstream-resolver-rs 2>/dev/null | tr " " "\n" | head -1)
        if [ -n "$PID" ]; then
            EXE=$(readlink /proc/$PID/exe 2>/dev/null)
            case "$EXE" in
                *"(deleted)")
                    echo "[updater] $(date +%FT%T) running process is on a replaced inode, bouncing" >> "$LOG"
                    kill $(pidof pstream-resolver-rs 2>/dev/null)
                    ;;
            esac
        else
            echo "[updater] $(date +%FT%T) no resolver process found" >> "$LOG"
        fi
    done
' > /dev/null 2>&1 < /dev/null &

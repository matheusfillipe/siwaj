# siwaj

Should i wear a jacket? This little e-paper sign answers before you finish asking.

It is a Waveshare ESP32-S3-ePaper-1.54 (V2) board with a 200x200 e-ink display. The firmware sleeps in a drawer, wakes up every half hour, asks OpenWeather how it feels outside, and draws one of four garments (jacket, pullover, shirt, t-shirt) plus a rain risk warning. Then it goes back to sleep for another half hour, which is why a small battery is designed to last months. If nobody configured it yet, it turns into a wifi hotspot and serves its own config page. Type your city, pick thresholds, done.

The name is short for "should i wear a jacket". The README is mostly the name.

## Burn it

You need the board above, an OpenWeather key (free "One Call by Call" plan) and the Rust esp toolchain ([espup](https://github.com/esp-rs/espup)).

```
make install
```

Put your secrets in `.env` (`OPENWEATHER_API_KEY=...`, `WIFI_SSID=...`, `WIFI_PASS=...`) and plug the board in over USB:

```
make provision       # push secrets into the device's encrypted storage
make firmware-flash  # build, flash, drop into the serial console
```

The console prints the device's address once wifi connects. Open it, set your city, and let it live somewhere you dress in front of. Holding BOOT while resetting forces the config page again.

## No device yet?

The whole device runs emulated, config page included:

```
make qemu-run        # emulated device on http://127.0.0.1:47652
make demo            # the e-paper face itself, in a window
```

`make help` lists the rest. Hardware notes, decisions and the full plan live in [docs/research.md](docs/research.md).

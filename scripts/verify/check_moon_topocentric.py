#!/usr/bin/env python3
"""Moon parallax acceptance: astropy is the truth, unisolver's topocentric correction is checked.

    python3 scripts/verify/check_moon_topocentric.py      # needs astropy

Criterion: **topocentric position within 0.1° of astropy**. It also prints the parallax and
the geocentric error, to tell "inherent error of the truncated series" apart from "a wrong
parallax correction", which are hard to separate in a single number.

The Rust side's positions come as JSON from `examples/probe_moon.rs`; this script provides
the truth and the comparison, so it also regenerates the anchors in `ephemeris.rs`.
"""
from __future__ import annotations

import json
import subprocess
import sys

TOL_DEG = 0.1

# (unix_ms, lat, lon, alt_m, label): both hemispheres, both longitudes, one high latitude
CASES = [
    (1788134400000, 31.2, 121.5, 10.0, "Shanghai"),
    (1760000000000, 31.2, 121.5, 10.0, "Shanghai"),
    (1800000000000, -33.9, 18.4, 100.0, "CapeTown"),
    (1704110400000, 64.0, -21.9, 50.0, "Reykjavik"),
]


def sep_deg(ra1: float, dec1: float, ra2: float, dec2: float) -> float:
    import math

    r1, d1, r2, d2 = map(math.radians, (ra1, dec1, ra2, dec2))
    c = math.sin(d1) * math.sin(d2) + math.cos(d1) * math.cos(d2) * math.cos(r1 - r2)
    return math.degrees(math.acos(max(-1.0, min(1.0, c))))


def main() -> int:
    try:
        from astropy.coordinates import EarthLocation, get_body
        from astropy.time import Time
        import astropy.units as u
    except ImportError:
        print(
            "astropy is required (pip install astropy), then:\n"
            "  python3 scripts/verify/check_moon_topocentric.py",
            file=sys.stderr,
        )
        return 2

    print(f"{'time/site':28s} {'parallax':>8s} {'geo err':>9s} {'topo err':>9s}")
    worst = 0.0
    anchors = []
    for ms, lat, lon, alt, tag in CASES:
        out = subprocess.run(
            [
                "cargo", "run", "--release", "-q", "-p", "unisolver-core",
                "--example", "probe_moon", "--",
                str(ms), str(lat), str(lon), str(alt),
            ],
            capture_output=True, text=True, check=True,
        ).stdout
        got = json.loads(out)

        t = Time(ms / 1000.0, format="unix")
        loc = EarthLocation(lat=lat * u.deg, lon=lon * u.deg, height=alt * u.m)
        geo, topo = get_body("moon", t), get_body("moon", t, location=loc)

        parallax = sep_deg(geo.ra.deg, geo.dec.deg, topo.ra.deg, topo.dec.deg)
        e_geo = sep_deg(got["geo"]["ra_deg"], got["geo"]["dec_deg"], geo.ra.deg, geo.dec.deg)
        e_topo = sep_deg(got["topo"]["ra_deg"], got["topo"]["dec_deg"], topo.ra.deg, topo.dec.deg)
        worst = max(worst, e_topo)
        flag = "" if e_topo < TOL_DEG else "  <- over the limit"
        print(f"{str(ms) + ' ' + tag:28s} {parallax:7.3f}° {e_geo:8.3f}° {e_topo:8.3f}°{flag}")
        anchors.append((ms, lat, lon, alt, topo.ra.deg, topo.dec.deg))

    print(f"\nworst topocentric error {worst:.3f}° (criterion < {TOL_DEG}°)")
    print("\nanchors for ephemeris.rs (astropy topocentric truth, ready to paste):")
    for ms, lat, lon, alt, ra, dec in anchors:
        print(f"        ({ms}, {lat}, {lon}, {alt}, {ra:.4f}, {dec:.4f}),")
    return 0 if worst < TOL_DEG else 1


if __name__ == "__main__":
    sys.exit(main())

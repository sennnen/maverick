# Standard-profile fixtures

Byte vectors for Bluetooth SIG standard characteristics, decoded by the admitted profile
decoders in `mav-codec/src/standard.rs`. Unlike the WHOOP fixtures these are not captures:
every case is constructed directly from the published SIG specification named in the file's
`source` field, so the confidence tag is `[PROV]` with the specification as the named source.

- `hr_measurement_v1.json` — Heart Rate Measurement (`0x2A37`, Heart Rate Service `0x180D`):
  u8/u16 heart-rate flag, sensor-contact bits ignored, energy-expended skip, RR intervals in
  1/1024 s converted to exact milliseconds (dyadic division — `816 → 796.875 ms` is exact in
  binary floating point), a zero heart rate treated as the no-reading sentinel, and exact
  truncation boundaries that must fail with `MAV-3003`.

The rules in `fixtures/README.md` apply: never hand-edit a case to make a test pass; a wrong
expectation means the case is rederived from the specification and the change explained in the
commit.

These descriptor fixtures preserve byte arrays already present in the local
repository history in commit `3a6c2b0e4a6cad10c57e48f09ddacbc98f44e5f0`:

- `rongyuan_m1_v5_if2.hex` comes from `M1_V5_IF2_DESC`.
- `beiying_tac75_if2.hex` comes from `RAID75_IF2_DESC`; its source comment
  identifies it as the captured TAC75 HE interface descriptor.

The files are hexadecimal renderings of captured report descriptors. Tests
parse these bytes directly and do not open HID devices.

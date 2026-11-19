# Protocol

## Versions

Every file carries a version vector; a device sends only the files whose vectors moved.

## Transport

Changes travel over HTTPS in batches of at most 64 files.

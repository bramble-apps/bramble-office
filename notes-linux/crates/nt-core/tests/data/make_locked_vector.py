"""Independent locked-note vector for tests/lock_test.rs, made with Python's `cryptography` (not our code).

Encrypts the research note "01 Plain" the way Notes stores a locked note (format section 5):
PBKDF2-HMAC-SHA256(password, salt, iterations, 16) wraps (RFC 3394) a random note key, and the note's
gzip protobuf is AES-128-GCM-encrypted with a 16-byte IV. Usage: python3 -I make_locked_vector.py STORE OUT.json
"""
import json, os, sqlite3, sys
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.hashes import SHA256
from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2HMAC
from cryptography.hazmat.primitives.keywrap import aes_key_wrap

store, out = sys.argv[1], sys.argv[2]
c = sqlite3.connect(f"file:{store}?mode=ro", uri=True)
ent = c.execute("SELECT Z_ENT FROM Z_PRIMARYKEY WHERE Z_NAME='ICNote'").fetchone()[0]
ident, data = c.execute(
    "SELECT n.ZIDENTIFIER, d.ZDATA FROM ZICCLOUDSYNCINGOBJECT n JOIN ZICNOTEDATA d ON d.Z_PK = n.ZNOTEDATA "
    "WHERE n.Z_ENT = ? AND n.ZTITLE1 = '01 Plain'", (ent,)).fetchone()
password, salt, iterations = "Bramble-NO1-test", bytes(range(16)), 20000
kek = PBKDF2HMAC(SHA256(), 16, salt, iterations).derive(password.encode())
key, iv = os.urandom(16), os.urandom(16)
ct = AESGCM(key).encrypt(iv, data, None)
json.dump({
    "password": password, "note": ident, "iterations": iterations, "salt": salt.hex(), "kek": kek.hex(),
    "wrapped_key": aes_key_wrap(kek, key).hex(), "iv": iv.hex(), "tag": ct[-16:].hex(), "data": ct[:-16].hex(),
    # a fixed GCM case: key 00..0f, iv 10..1f, "Bramble"
    "gcm": {"ct_tag": AESGCM(bytes(range(16))).encrypt(bytes(range(16, 32)), b"Bramble", None).hex()},
}, open(out, "w"), indent=1)

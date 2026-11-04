"""Open the locked notes of a NoteStore.sqlite with a password, independently of nt-core (Python's
`cryptography`; scheme as in apple_cloud_notes_parser). Prints each note's id and the gunzipped body's
length and a hash, plus each locked attachment's decrypted values JSON keys.
Usage: python3 -I open_locked.py NoteStore.sqlite PASSWORD"""
import gzip, hashlib, json, sqlite3, sys
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.hashes import SHA256
from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2HMAC
from cryptography.hazmat.primitives.keywrap import aes_key_unwrap

db, pw = sys.argv[1], sys.argv[2].encode()
c = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
ents = dict(c.execute("SELECT Z_NAME, Z_ENT FROM Z_PRIMARYKEY"))

def key(salt, it, wrapped):
    return aes_key_unwrap(PBKDF2HMAC(SHA256(), 16, salt, it).derive(pw), wrapped)

def gcm(k, iv, tag, data):
    return AESGCM(k).decrypt(iv, data + tag, None)

for pk, ident, salt, it, wk in c.execute(
        "SELECT Z_PK, ZIDENTIFIER, ZCRYPTOSALT, ZCRYPTOITERATIONCOUNT, ZCRYPTOWRAPPEDKEY FROM ZICCLOUDSYNCINGOBJECT "
        "WHERE Z_ENT = ? AND ZISPASSWORDPROTECTED = 1", (ents["ICNote"],)).fetchall():
    k = key(salt, it, wk)
    data, iv, tag = c.execute("SELECT ZDATA, ZCRYPTOINITIALIZATIONVECTOR, ZCRYPTOTAG FROM ZICNOTEDATA WHERE ZNOTE = ?", (pk,)).fetchone()
    body = gzip.decompress(gcm(k, iv, tag, data))
    print("note", ident, len(body), hashlib.sha256(body).hexdigest()[:16])
    for aid, salt, it, wk, v, iv, tag in c.execute(
            "SELECT ZIDENTIFIER, ZCRYPTOSALT, ZCRYPTOITERATIONCOUNT, ZCRYPTOWRAPPEDKEY, ZENCRYPTEDVALUESJSON, "
            "ZCRYPTOINITIALIZATIONVECTOR, ZCRYPTOTAG FROM ZICCLOUDSYNCINGOBJECT WHERE ZNOTE = ? AND Z_ENT = ? "
            "AND ZISPASSWORDPROTECTED = 1 ORDER BY ZIDENTIFIER", (pk, ents["ICAttachment"])).fetchall():
        print("attachment", aid, sorted(json.loads(gcm(key(salt, it, wk), iv, tag, v))))

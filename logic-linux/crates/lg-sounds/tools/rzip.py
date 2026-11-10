"""Remote zip access over HTTP range requests: list members, extract selected ones.

usage: rzip.py list URL
       rzip.py get URL OUTDIR REGEX [REGEX...]
Only reads data; never executes anything from the archive.
"""
import io, os, re, sys, urllib.request, zipfile


class RangeFile(io.RawIOBase):
    def __init__(self, url):
        self.url = url
        req = urllib.request.Request(url, method="HEAD")
        with urllib.request.urlopen(req) as r:
            self.size = int(r.headers["Content-Length"])
            self.url = r.url
        self.pos = 0
        self.cache = {}  # block -> bytes
        self.block = 1 << 20

    def seekable(self):
        return True

    def readable(self):
        return True

    def seek(self, off, whence=0):
        if whence == 0:
            self.pos = off
        elif whence == 1:
            self.pos += off
        else:
            self.pos = self.size + off
        return self.pos

    def tell(self):
        return self.pos

    def _fetch(self, a, b):
        req = urllib.request.Request(self.url, headers={"Range": f"bytes={a}-{b - 1}"})
        for attempt in range(5):
            try:
                with urllib.request.urlopen(req, timeout=120) as r:
                    return r.read()
            except Exception as e:  # retry
                err = e
        raise err

    def read(self, n=-1):
        if n < 0:
            n = self.size - self.pos
        n = max(0, min(n, self.size - self.pos))
        if n == 0:
            return b""
        if n > 4 * self.block:
            data = self._fetch(self.pos, self.pos + n)
            self.pos += len(data)
            return data
        out = bytearray()
        while n > 0:
            bi = self.pos // self.block
            if bi not in self.cache:
                a = bi * self.block
                self.cache = {bi: self._fetch(a, min(a + self.block, self.size))} | (
                    {k: v for k, v in list(self.cache.items())[-8:]})
            blk = self.cache[bi]
            o = self.pos - bi * self.block
            take = blk[o:o + n]
            out += take
            self.pos += len(take)
            n -= len(take)
        return bytes(out)


def open_remote(url):
    return zipfile.ZipFile(RangeFile(url))


def extract(z, out, names=None, pats=()):
    """Extracts the members named in `names` or matching one of the regexes `pats`."""
    pats = [re.compile(p) if isinstance(p, str) else p for p in pats]
    names = set(names or ())
    n = 0
    for i in z.infolist():
        if i.is_dir() or not (i.filename in names or any(p.search(i.filename) for p in pats)):
            continue
        name = os.path.normpath(i.filename)
        if name.startswith("..") or os.path.isabs(name):
            continue
        dst = os.path.join(out, name)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        with z.open(i) as src, open(dst, "wb") as f:
            while True:
                b = src.read(1 << 20)
                if not b:
                    break
                f.write(b)
        n += 1
    return n


def extract_parallel(z, out, names, workers=6):
    """Like `extract` for a list of names, fetching members over parallel range requests."""
    import struct
    import zlib
    from concurrent.futures import ThreadPoolExecutor
    rf = z.fp
    url = rf.url if hasattr(rf, "url") else z.filename

    def one(info):
        name = os.path.normpath(info.filename)
        if name.startswith("..") or os.path.isabs(name):
            return 0
        f = RangeFile.__new__(RangeFile)
        f.url, f.size, f.pos, f.cache, f.block = url, rf.size, 0, {}, 1 << 20
        hdr = f._fetch(info.header_offset, info.header_offset + 30)
        n, m = struct.unpack("<HH", hdr[26:30])
        start = info.header_offset + 30 + n + m
        data = f._fetch(start, start + info.compress_size) if info.compress_size else b""
        if info.compress_type == zipfile.ZIP_DEFLATED:
            data = zlib.decompress(data, -15)
        elif info.compress_type != zipfile.ZIP_STORED:
            raise ValueError(f"unsupported compression {info.compress_type}")
        if zlib.crc32(data) & 0xFFFFFFFF != info.CRC:
            raise ValueError(f"CRC mismatch: {info.filename}")
        dst = os.path.join(out, name)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        with open(dst, "wb") as fo:
            fo.write(data)
        return 1

    infos = [z.getinfo(n) for n in names]
    with ThreadPoolExecutor(workers) as ex:
        return sum(ex.map(one, infos))


def main():
    cmd, url = sys.argv[1], sys.argv[2]
    z = open_remote(url)
    if cmd == "list":
        for i in z.infolist():
            print(i.file_size, i.filename)
        return
    print("extracted", extract(z, sys.argv[3], pats=sys.argv[4:]))


if __name__ == "__main__":
    main()

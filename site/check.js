const K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
]);

const H0 = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

function rotr(x, n) {
  return (x >>> n) | (x << (32 - n));
}

class Sha256 {
  constructor() {
    this.h = Uint32Array.from(H0);
    this.buf = new Uint8Array(64);
    this.bufLen = 0;
    this.total = 0;
  }

  update(data) {
    let off = 0;
    this.total += data.length;
    if (this.bufLen > 0) {
      const take = Math.min(64 - this.bufLen, data.length);
      this.buf.set(data.subarray(0, take), this.bufLen);
      this.bufLen += take;
      off = take;
      if (this.bufLen === 64) {
        this.block(this.buf, 0);
        this.bufLen = 0;
      }
    }
    while (data.length - off >= 64) {
      this.block(data, off);
      off += 64;
    }
    if (off < data.length) {
      this.buf.set(data.subarray(off), 0);
      this.bufLen = data.length - off;
    }
  }

  block(b, o) {
    const w = new Uint32Array(64);
    for (let i = 0; i < 16; i++) {
      w[i] = (b[o + 4 * i] << 24) | (b[o + 4 * i + 1] << 16) | (b[o + 4 * i + 2] << 8) | b[o + 4 * i + 3];
    }
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }
    let [a, bb, c, d, e, f, g, h] = this.h;
    for (let i = 0; i < 64; i++) {
      const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (h + s1 + ch + K[i] + w[i]) >>> 0;
      const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & bb) ^ (a & c) ^ (bb & c);
      const t2 = (s0 + maj) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = bb;
      bb = a;
      a = (t1 + t2) >>> 0;
    }
    const v = [a, bb, c, d, e, f, g, h];
    for (let i = 0; i < 8; i++) this.h[i] = (this.h[i] + v[i]) >>> 0;
  }

  hex() {
    const bits = this.total * 8;
    const tail = new Uint8Array(128);
    tail.set(this.buf.subarray(0, this.bufLen), 0);
    tail[this.bufLen] = 0x80;
    const end = this.bufLen < 56 ? 64 : 128;
    const hi = Math.floor(bits / 4294967296);
    const lo = bits >>> 0;
    tail[end - 8] = (hi >>> 24) & 0xff;
    tail[end - 7] = (hi >>> 16) & 0xff;
    tail[end - 6] = (hi >>> 8) & 0xff;
    tail[end - 5] = hi & 0xff;
    tail[end - 4] = (lo >>> 24) & 0xff;
    tail[end - 3] = (lo >>> 16) & 0xff;
    tail[end - 2] = (lo >>> 8) & 0xff;
    tail[end - 1] = lo & 0xff;
    this.block(tail, 0);
    if (end === 128) this.block(tail, 64);
    return Array.from(this.h, (x) => x.toString(16).padStart(8, "0")).join("");
  }
}

async function sha256File(file, onProgress) {
  const hasher = new Sha256();
  const reader = file.stream().getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    hasher.update(value);
    onProgress(hasher.total, file.size);
  }
  return hasher.hex();
}

const form = document.querySelector("#check-form");
const fileInput = document.querySelector("#file");
const expectedInput = document.querySelector("#expected");
const progress = document.querySelector("#progress");
const result = document.querySelector("#result");

function show(text, kind) {
  result.textContent = text;
  result.className = kind ? `result ${kind}` : "result";
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  const file = fileInput.files[0];
  const want = expectedInput.value.trim().toLowerCase();
  if (!file) {
    show("pick a file first", "error");
    return;
  }
  if (!/^[0-9a-f]{64}$/.test(want)) {
    show("expected hash must be 64 hex characters", "error");
    return;
  }
  show("");
  const got = await sha256File(file, (done, total) => {
    progress.textContent = `hashing ${file.name}: ${Math.floor((done / total) * 100)}%`;
  });
  progress.textContent = "";
  if (got === want) {
    show("match: the file is the one that was published", "ok");
  } else {
    show(`mismatch. expected ${want}, got ${got}`, "error");
  }
});

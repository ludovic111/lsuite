// A tiny tar writer (POSIX ustar) for the marketplace's tests, so they don't depend on a tar on
// the machine (CI runs on Ubuntu, the owner on macOS). Not a test file itself.
import { gzipSync } from 'node:zlib';

/** One 512-byte ustar header. Paths over 100 bytes go to `prefix` at a `/`. */
export function tarHeader(path, size, type = '0', link = '') {
  const h = Buffer.alloc(512);
  let name = path;
  let prefix = '';
  if (Buffer.byteLength(name) > 100) {
    const cut = path.lastIndexOf('/', 155);
    prefix = path.slice(0, cut);
    name = path.slice(cut + 1);
  }
  const put = (value, at, length) => h.write(value, at, length, 'utf8');
  const octal = (n, length) => n.toString(8).padStart(length - 1, '0') + '\0';
  put(name, 0, 100);
  put(octal(type === '5' ? 0o755 : 0o644, 8), 100, 8);
  put(octal(0, 8), 108, 8);
  put(octal(0, 8), 116, 8);
  put(octal(size, 12), 124, 12);
  put(octal(1_760_000_000, 12), 136, 12);
  put('        ', 148, 8);
  put(type, 156, 1);
  put(link, 157, 100);
  put('ustar\0', 257, 6);
  put('00', 263, 2);
  put(prefix, 345, 155);
  let sum = 0;
  for (const b of h) sum += b;
  put(`${sum.toString(8).padStart(6, '0')}\0 `, 148, 8);
  return h;
}

/** A .tar.gz of `entries`: `{path, data}` files, `{path, type: '5'}` folders, `{path, type: '2', link}` links. */
export function targz(entries) {
  const blocks = [];
  for (const e of entries) {
    const data = e.data === undefined ? Buffer.alloc(0) : Buffer.from(e.data);
    blocks.push(tarHeader(e.path, data.length, e.type ?? '0', e.link ?? ''), data, Buffer.alloc((512 - (data.length % 512)) % 512));
  }
  blocks.push(Buffer.alloc(1024));
  return gzipSync(Buffer.concat(blocks));
}

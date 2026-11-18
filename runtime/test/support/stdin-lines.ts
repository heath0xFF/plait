/** JSON lines end at LF; Node's readline also splits Unicode separators inside JSON strings. */
export function stdinLines(line: (value: string) => void, closed: () => void) {
  let buffered = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', chunk => {
    buffered += chunk;
    for (let end = buffered.indexOf('\n'); end >= 0; end = buffered.indexOf('\n')) {
      const value = buffered.slice(0, end);
      buffered = buffered.slice(end + 1);
      line(value);
    }
  });
  process.stdin.on('end', closed);
}

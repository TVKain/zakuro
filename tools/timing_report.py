"""reads a ZAKURO_TIMING record and reports input lag, frame pacing and audio sync.

  python tools/timing_report.py <record> [first frame] [last frame]

frames outside the range are left out, for example the boot and menus before
a replay starts walking.

display model: a present that finished at p reaches the screen at the first
vblank after p (lower bound; DWM composition in a window adds one refresh on
top, the same for every variant, so comparisons hold).
"""
import sys, statistics as st, bisect

def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(q * len(xs)))]

def main(path, first=0, last=10**12):
    E, P, A = [], [], {}
    freq = 1
    for line in open(path):
        w = line.split()
        if w[0] == 'F':
            freq = int(w[1])
        elif w[0] == 'E':
            s, f, d, n = map(int, w[1:5])
            E.append((s, f, d, n))
        elif w[0] == 'A':
            a, h, n = map(int, w[1:4])
            A[n] = (a, h)
        elif w[0] == 'P':
            s, f, n, vb, per = map(int, w[1:6])
            P.append((s, f, n, vb, per))
    ms = lambda t: t * 1000.0 / freq
    E = [e for e in E if first <= e[3] <= last]
    if not E:
        print("no frames"); return
    lo, hi = E[0][0], E[-1][1]
    P = [p for p in P if lo <= p[0] <= hi + freq]
    per = st.median([p[4] for p in P if p[4]]) if any(p[4] for p in P) else 0

    def shown_at(p):
        s, f, n, vb, pr = p
        if not pr:
            return f
        k = -(-(f - vb) // pr)  # first vblank after f
        return vb + k * pr

    pstarts = [p[0] for p in P]
    lat, emu, wait, pres, vs, late, disp = [], [], [], [], [], [], []
    dropped = 0
    sync, alat = [], []
    for (s, f, d, n) in E:
        late.append(ms(s - d))
        i = bisect.bisect_left(pstarts, f)
        while i < len(P) and P[i][2] < n:
            i += 1
        if i == len(P) or P[i][2] != n:
            dropped += 1  # a later frame was emulated before any present
            continue
        p = P[i]
        t = shown_at(p)
        emu.append(ms(f - s)); wait.append(ms(p[0] - f)); pres.append(ms(p[1] - p[0])); vs.append(ms(t - p[1]))
        lat.append(ms(t - s)); disp.append(t)
        if n in A:
            sync.append(ms(A[n][1] - t)); alat.append(ms(A[n][1] - s))
    iv = [ms(b - a) for a, b in zip(disp, disp[1:])]
    print(f"{path}")
    print(f"  frames {len(E)}  shown {len(disp)}  dropped {dropped}  presents {len(P)}  refresh {ms(per):.3f} ms")
    def row(name, xs):
        print(f"  {name:28s} mean {st.mean(xs):6.2f}  p50 {pct(xs,.5):6.2f}  p95 {pct(xs,.95):6.2f}  p99 {pct(xs,.99):6.2f}  max {max(xs):6.2f} ms")
    row("emulate start late", late)
    row("emulate (input->ready)", emu)
    row("ready -> present call", wait)
    row("present call", pres)
    row("present -> vblank", vs)
    row("input read -> vblank", lat)
    row("shown interval", iv)
    if per:
        counts = {}
        for x in iv:
            k = round(x / ms(per))
            counts[k] = counts.get(k, 0) + 1
        tot = sum(counts.values())
        mode = max(counts, key=counts.get)
        print(f"  shown interval in refreshes  " + "  ".join(f"{k}:{v}" for k, v in sorted(counts.items())) + f"   off-beat {100*(tot-counts[mode])/tot:.2f}%")
    print(f"  shown interval stdev {st.pstdev(iv):.3f} ms")
    if sync:
        row("input read -> heard", alat)
        row("sound after picture", sync)

if __name__ == '__main__':
    a = sys.argv[1:]
    main(a[0], int(a[1]) if len(a) > 1 else 0, int(a[2]) if len(a) > 2 else 10**12)

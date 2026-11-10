Machine: 13th Gen Intel(R) Core(TM) i9-13900K, pinned to 1 CPU; pdfTeX 3.141592653-2.6-1.40.29 (TeX Live 2026); image prumo-dev:tl2026-full. Warm medians of 5 runs unless the heading says otherwise; the cold run has a fresh HOME.

### minimal

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 42.3 | 84.0 | 242.8 |
| preamble | 8.2 | 46.0 | 22.9 |
| begin-document | 1.8 | 2.0 | 2.2 |
| typesetting | 0.1 | 0.1 | 0.1 |
| shipout | 45.3 | 0.2 | 35.9 |
| end-document | 0.7 | 0.6 | 0.7 |
| finalization | 41.1 | 146.8 | 19.1 |
| **total (wall)** | **140.0** | **279.1** | **324.7** |
| spread (min–max) | 137–142 | 275–285 | 322–329 |
| cold run (wall) | 144.2 | 3,273.8 | 4,629.5 |
| pages | 1 | 1 | 1 |

### paper

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 43.0 | 83.8 | 247.9 |
| preamble | 499.8 | 719.9 | 780.5 |
| begin-document | 49.7 | 48.2 | 54.4 |
| typesetting | 150.6 | 270.5 | 211.0 |
| shipout | 60.5 | 31.1 | 64.3 |
| end-document | 2.4 | 3.8 | 3.5 |
| finalization | 54.3 | 58.1 | 71.3 |
| **total (wall)** | **859.6** | **1,219.4** | **1,431.5** |
| spread (min–max) | 855–871 | 1,205–1,232 | 1,388–1,439 |
| cold run (wall) | 863.6 | 4,193.7 | 5,977.3 |
| pages | 5 | 5 | 5 |

### generated book (3 warm runs)

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 44.3 | 83.4 | 242.5 |
| preamble | 192.3 | 283.8 | 273.7 |
| begin-document | 58.6 | 49.0 | 77.3 |
| typesetting | 14,322.1 | 15,935.5 | 27,305.0 |
| shipout | 2,872.5 | 3,831.2 | 10,949.3 |
| end-document | 25.0 | 22.4 | 24.9 |
| finalization | 119.6 | 230.7 | 157.7 |
| **total (wall)** | **17,643.5** | **20,433.3** | **39,037.4** |
| spread (min–max) | 17,078–17,716 | 20,412–20,688 | 39,004–39,902 |
| cold run (wall) | 17,405.7 | 23,292.1 | 44,183.2 |
| pages | 3991 | 3991 | 3991 |

pdflatex: 8,669 records, 164 KiB. `prumo time --json` reads the cached run in 3 ms, and `--html` writes 1,279 KiB in 8 ms.

xelatex: 8,661 records, 165 KiB. `prumo time --json` reads the cached run in 3 ms, and `--html` writes 1,277 KiB in 8 ms.

lualatex: 8,668 records, 169 KiB. `prumo time --json` reads the cached run in 3 ms, and `--html` writes 1,280 KiB in 8 ms.

### florilegium manual

| phase (ms) | lualatex |
|---|---:|
| startup | 245.8 |
| preamble | 294.3 |
| begin-document | 12.7 |
| typesetting | 519.1 |
| shipout | 473.5 |
| end-document | 2.2 |
| finalization | 92.4 |
| **total (wall)** | **1,633.2** |
| spread (min–max) | 1,615–1,663 |
| cold run (wall) | 6,163.4 |
| pages | 35 |

### ACM paper

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 41.6 | 82.7 | 242.2 |
| preamble | 749.5 | 1,145.6 | 1,515.8 |
| begin-document | 58.2 | 56.9 | 96.3 |
| typesetting | 995.5 | 1,136.0 | 1,300.8 |
| shipout | 274.0 | 454.7 | 283.9 |
| end-document | 258.3 | 7.5 | 146.7 |
| finalization | 71.4 | 70.0 | 146.2 |
| **total (wall)** | **2,451.0** | **2,951.1** | **3,726.2** |
| spread (min–max) | 2,420–2,518 | 2,943–2,976 | 3,713–3,837 |
| cold run (wall) | 2,469.5 | 5,948.5 | 10,267.5 |
| pages | 7 | 7 | 6 |

### ACM paper, biblatex

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 42.2 | 82.8 | 238.2 |
| preamble | 936.8 | 1,487.6 | 1,845.2 |
| begin-document | 120.4 | 128.8 | 166.0 |
| typesetting | 1,454.2 | 1,650.9 | 1,765.3 |
| shipout | 302.5 | 559.8 | 299.5 |
| end-document | 289.7 | 16.1 | 164.3 |
| finalization | 72.4 | 67.5 | 142.6 |
| **total (wall)** | **3,217.8** | **3,992.9** | **4,638.7** |
| spread (min–max) | 3,215–3,253 | 3,900–4,002 | 4,615–4,803 |
| cold run (wall) | 3,215.5 | 6,905.8 | 11,349.9 |
| pages | 8 | 8 | 7 |

### beamer deck

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 42.1 | 82.4 | 240.5 |
| preamble | 355.2 | 499.8 | 565.3 |
| begin-document | 29.7 | 27.8 | 37.5 |
| typesetting | 592.3 | 729.1 | 765.6 |
| shipout | 55.4 | 15.7 | 59.3 |
| end-document | 0.8 | 0.7 | 0.7 |
| finalization | 47.1 | 60.5 | 87.8 |
| **total (wall)** | **1,122.5** | **1,415.5** | **1,753.4** |
| spread (min–max) | 1,120–1,126 | 1,413–1,420 | 1,744–1,764 |
| cold run (wall) | 1,123.7 | 4,317.2 | 10,903.7 |
| pages | 33 | 33 | 33 |

### HoTT book

| phase (ms) | pdflatex |
|---|---:|
| startup | 42.9 |
| preamble | 284.1 |
| begin-document | 29.8 |
| typesetting | 2,355.2 |
| shipout | 1,304.0 |
| end-document | 32.4 |
| finalization | 61.4 |
| **total (wall)** | **4,110.1** |
| spread (min–max) | 4,103–4,112 |
| cold run (wall) | 4,112.4 |
| pages | 492 |

### Open Logic (3 warm runs)

| phase (ms) | pdflatex | xelatex | lualatex |
|---|---:|---:|---:|
| startup | 42.9 | 83.4 | 241.7 |
| preamble | 479.2 | 512.0 | 534.7 |
| begin-document | 52.2 | 41.7 | 59.0 |
| typesetting | 29,050.9 | 29,592.5 | 34,840.7 |
| shipout | 781.3 | 1,028.1 | 3,577.1 |
| end-document | 39.5 | 37.7 | 42.0 |
| finalization | 82.7 | 159.2 | 162.4 |
| **total (wall)** | **30,530.3** | **31,454.3** | **39,431.6** |
| spread (min–max) | 30,179–30,672 | 31,450–31,493 | 39,057–39,920 |
| cold run (wall) | 30,466.3 | 34,395.1 | 43,067.6 |
| pages | 1013 | 1011 | 1011 |

### Stacks Project (2 warm runs)

| phase (ms) | pdflatex |
|---|---:|
| startup | 41.8 |
| preamble | 111.2 |
| begin-document | 180.1 |
| typesetting | 38,218.8 |
| shipout | 5,330.1 |
| end-document | 43.3 |
| finalization | 1,057.0 |
| **total (wall)** | **44,982.3** |
| spread (min–max) | 44,961–45,003 |
| cold run (wall) | 45,053.1 |
| pages | 7710 |

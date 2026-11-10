| document | engine | pages | baseline | preamble | draft | externalize |
|---|---|---:|---:|---|---|---|
| minimal | pdflatex | 1 | 134 ms | noise | noise | – |
| minimal | xelatex | 1 | 271 ms | fails | 104 ms (38%) | – |
| minimal | lualatex | 1 | 317 ms | fails | noise | – |
| paper | pdflatex | 5 | 820 ms | 405 ms (49%) | 12 ms (2%) | 56 ms (7%) |
| paper | xelatex | 5 | 1,177 ms | fails | 150 ms (13%) | 59 ms (5%) |
| paper | lualatex | 5 | 1,333 ms | fails | 52 ms (4%) | 45 ms (3%) |
| generated book | pdflatex | 3,991 | 16,566 ms | noise | 1,112 ms (7%) | 745 ms (4%) |
| generated book | xelatex | 3,991 | 19,914 ms | fails | 3,156 ms (16%) | 286 ms (1%) |
| generated book | lualatex | 3,991 | 38,925 ms | fails | 2,495 ms (6%) | 672 ms (2%) |
| florilegium manual | lualatex | 35 | 1,632 ms | fails | 417 ms (26%) | 518 ms (32%) |
| ACM paper | pdflatex | 7 | 2,461 ms | fails | noise | – |
| ACM paper | xelatex | 7 | 3,012 ms | fails | 186 ms (6%) | – |
| ACM paper | lualatex | 6 | 3,846 ms | fails | noise | – |
| ACM paper, biblatex | pdflatex | 8 | 3,315 ms | fails | noise | – |
| ACM paper, biblatex | xelatex | 8 | 3,956 ms | fails | 183 ms (5%) | – |
| ACM paper, biblatex | lualatex | 7 | 4,641 ms | fails | noise | – |
| beamer deck | pdflatex | 33 | 1,098 ms | 278 ms (25%) | noise | 218 ms (20%) |
| beamer deck | xelatex | 33 | 1,398 ms | fails | 158 ms (11%) | 219 ms (16%), output changed |
| beamer deck | lualatex | 33 | 1,737 ms | fails | 70 ms (4%) | 264 ms (15%), output changed |
| HoTT book | pdflatex | 492 | 4,147 ms | fails | 1,043 ms (25%) | 171 ms (4%) |
| Open Logic | pdflatex | 1,013 | 31,190 ms | fails | 261 ms (1%) | 3,936 ms (13%) |
| Open Logic | xelatex | 1,011 | 32,550 ms | fails | noise | noise |
| Open Logic | lualatex | 1,011 | 41,563 ms | fails | 545 ms (1%) | 4,069 ms (10%) |
| Stacks Project | pdflatex | 7,710 | 45,605 ms | noise | 3,374 ms (7%) | – |

Failures and changed output:

- minimal, xelatex, preamble: could not precompile: ! Can't \dump a format with native fonts or font-mappings.
- minimal, lualatex, preamble: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key').
- paper, xelatex, preamble: could not precompile: ! Can't \dump a format with native fonts or font-mappings.
- paper, lualatex, preamble: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key').
- generated book, xelatex, preamble: could not precompile: ! Can't \dump a format with native fonts or font-mappings.
- generated book, lualatex, preamble: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key').
- florilegium manual, lualatex, preamble: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key').
- ACM paper, pdflatex, preamble: could not precompile: ! LaTeX Error: \begin{document} ended by \end{acks}.
- ACM paper, xelatex, preamble: could not precompile: ! LaTeX Error: \begin{document} ended by \end{acks}.
- ACM paper, lualatex, preamble: could not precompile: ! LaTeX Error: \begin{document} ended by \end{acks}.
- ACM paper, biblatex, pdflatex, preamble: could not precompile: ! Missing number, treated as zero.
- ACM paper, biblatex, xelatex, preamble: could not precompile: ! Missing number, treated as zero.
- ACM paper, biblatex, lualatex, preamble: could not precompile: ! Missing number, treated as zero.
- beamer deck, xelatex, preamble: could not precompile: ! Can't \dump a format with native fonts or font-mappings.
- beamer deck, xelatex, externalize: output changed, page 19 looks different (96 of 11011 pixels at 24 dpi; same words)
- beamer deck, lualatex, preamble: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key').
- beamer deck, lualatex, externalize: output changed, page 19 looks different (96 of 11011 pixels at 24 dpi; same words)
- HoTT book, pdflatex, preamble: the build on the precompiled preamble failed: ! File ended while scanning use of ^^M.
- Open Logic, pdflatex, preamble: could not precompile: ! Paragraph ended before \openout was complete.
- Open Logic, xelatex, preamble: could not precompile: ! Paragraph ended before \openout was complete.
- Open Logic, lualatex, preamble: could not precompile: ! Paragraph ended before \openout was complete.

Every row (ms; ± is the spread, slowest run minus fastest):

| document | engine | experiment | baseline | variant | saved | bound | one-time | output |
|---|---|---|---:|---:|---:|---:|---:|---|
| minimal | pdflatex | preamble | 133.8 ± 4.5 | 131.4 ± 7.3 | 2.5 | 9 | 169 | identical |
| minimal | pdflatex | draft | 133.8 ± 4.5 | 134.9 ± 5.2 | -1.1 |  |  | not-compared |
| minimal | pdflatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| minimal | xelatex | preamble | | | failed: could not precompile: ! Can't \dump a format with native fonts or font-mappings. | 47 | | |
| minimal | xelatex | draft | 271.4 ± 16.2 | 167.3 ± 13.1 | 104.0 |  |  | not-compared |
| minimal | xelatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| minimal | lualatex | preamble | | | failed: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key'). | 22 | | |
| minimal | lualatex | draft | 317.3 ± 12.0 | 312.9 ± 16.2 | 4.4 |  |  | not-compared |
| minimal | lualatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| paper | pdflatex | preamble | 820.0 ± 9.4 | 414.7 ± 14.0 | 405.3 | 503 | 739 | identical |
| paper | pdflatex | draft | 820.0 ± 9.4 | 807.9 ± 10.8 | 12.0 |  |  | not-compared |
| paper | pdflatex | externalize | 820.0 ± 9.4 | 763.8 ± 7.6 | 56.2 | 58 | 2,191 | identical |
| paper | xelatex | preamble | | | failed: could not precompile: ! Can't \dump a format with native fonts or font-mappings. | 723 | | |
| paper | xelatex | draft | 1,176.6 ± 19.6 | 1,026.4 ± 13.9 | 150.2 |  |  | not-compared |
| paper | xelatex | externalize | 1,176.6 ± 19.6 | 1,118.0 ± 31.7 | 58.6 | 124 | 3,251 | identical |
| paper | lualatex | preamble | | | failed: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key'). | 740 | | |
| paper | lualatex | draft | 1,332.8 ± 8.7 | 1,280.7 ± 25.6 | 52.1 |  |  | not-compared |
| paper | lualatex | externalize | 1,332.8 ± 8.7 | 1,287.3 ± 10.5 | 45.5 | 55 | 3,660 | identical |
| generated book | pdflatex | preamble | 16,565.8 ± 123.1 | 16,443.9 ± 32.3 | 122.0 | 183 | 364 | identical |
| generated book | pdflatex | draft | 16,565.8 ± 123.1 | 15,454.0 ± 404.7 | 1,111.8 |  |  | not-compared |
| generated book | pdflatex | externalize | 16,565.8 ± 123.1 | 15,821.1 ± 371.7 | 744.7 | 537 | 1,863,898 | identical |
| generated book | xelatex | preamble | | | failed: could not precompile: ! Can't \dump a format with native fonts or font-mappings. | 277 | | |
| generated book | xelatex | draft | 19,914.0 ± 85.9 | 16,758.4 ± 66.5 | 3,155.6 |  |  | not-compared |
| generated book | xelatex | externalize | 19,914.0 ± 85.9 | 19,628.1 ± 8.4 | 286.0 | 531 | 2,105,994 | identical |
| generated book | lualatex | preamble | | | failed: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key'). | 258 | | |
| generated book | lualatex | draft | 38,925.3 ± 344.5 | 36,430.6 ± 203.1 | 2,494.7 |  |  | not-compared |
| generated book | lualatex | externalize | 38,925.3 ± 344.5 | 38,253.4 ± 204.7 | 671.9 | 551 | 3,772,639 | identical |
| florilegium manual | lualatex | preamble | | | failed: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key'). | 286 | | |
| florilegium manual | lualatex | draft | 1,632.2 ± 87.8 | 1,215.1 ± 163.5 | 417.2 |  |  | not-compared |
| florilegium manual | lualatex | externalize | 1,632.2 ± 87.8 | 1,114.7 ± 220.3 | 517.5 | 171 | 2,498 | identical |
| ACM paper | pdflatex | preamble | | | failed: could not precompile: ! LaTeX Error: \begin{document} ended by \end{acks}. | 786 | | |
| ACM paper | pdflatex | draft | 2,461.4 ± 90.5 | 2,411.9 ± 102.6 | 49.6 |  |  | not-compared |
| ACM paper | pdflatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| ACM paper | xelatex | preamble | | | failed: could not precompile: ! LaTeX Error: \begin{document} ended by \end{acks}. | 1,180 | | |
| ACM paper | xelatex | draft | 3,012.1 ± 60.9 | 2,826.2 ± 77.8 | 186.0 |  |  | not-compared |
| ACM paper | xelatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| ACM paper | lualatex | preamble | | | failed: could not precompile: ! LaTeX Error: \begin{document} ended by \end{acks}. | 1,555 | | |
| ACM paper | lualatex | draft | 3,846.1 ± 470.3 | 3,676.5 ± 210.0 | 169.6 |  |  | not-compared |
| ACM paper | lualatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| ACM paper, biblatex | pdflatex | preamble | | | failed: could not precompile: ! Missing number, treated as zero. | 954 | | |
| ACM paper, biblatex | pdflatex | draft | 3,315.0 ± 134.9 | 3,267.8 ± 157.3 | 47.3 |  |  | not-compared |
| ACM paper, biblatex | pdflatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| ACM paper, biblatex | xelatex | preamble | | | failed: could not precompile: ! Missing number, treated as zero. | 1,529 | | |
| ACM paper, biblatex | xelatex | draft | 3,955.9 ± 76.3 | 3,773.2 ± 71.6 | 182.7 |  |  | not-compared |
| ACM paper, biblatex | xelatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| ACM paper, biblatex | lualatex | preamble | | | failed: could not precompile: ! Missing number, treated as zero. | 1,865 | | |
| ACM paper, biblatex | lualatex | draft | 4,640.8 ± 52.8 | 4,529.4 ± 120.4 | 111.5 |  |  | not-compared |
| ACM paper, biblatex | lualatex | externalize | | | not-run: the document does not load tikz | 0 | | |
| beamer deck | pdflatex | preamble | 1,098.3 ± 12.4 | 820.6 ± 14.3 | 277.7 | 364 | 554 | identical |
| beamer deck | pdflatex | draft | 1,098.3 ± 12.4 | 1,088.8 ± 6.8 | 9.4 |  |  | not-compared |
| beamer deck | pdflatex | externalize | 1,098.3 ± 12.4 | 880.6 ± 13.9 | 217.7 | 228 | 7,508 | identical |
| beamer deck | xelatex | preamble | | | failed: could not precompile: ! Can't \dump a format with native fonts or font-mappings. | 514 | | |
| beamer deck | xelatex | draft | 1,397.9 ± 28.6 | 1,240.0 ± 9.7 | 157.9 |  |  | not-compared |
| beamer deck | xelatex | externalize | 1,397.9 ± 28.6 | 1,179.3 ± 14.9 | 218.6 | 240 | 10,673 | changed |
| beamer deck | lualatex | preamble | | | failed: could not precompile: lua-uni-stage-tables.lua:141: attempt to perform arithmetic on a string value (local 'key'). | 595 | | |
| beamer deck | lualatex | draft | 1,736.8 ± 21.3 | 1,667.3 ± 21.7 | 69.5 |  |  | not-compared |
| beamer deck | lualatex | externalize | 1,736.8 ± 21.3 | 1,472.9 ± 39.7 | 263.9 | 270 | 12,537 | changed |
| HoTT book | pdflatex | preamble | | | failed: the build on the precompiled preamble failed: ! File ended while scanning use of ^^M. | 293 | | |
| HoTT book | pdflatex | draft | 4,146.9 ± 100.5 | 3,103.7 ± 68.1 | 1,043.2 |  |  | not-compared |
| HoTT book | pdflatex | externalize | 4,146.9 ± 100.5 | 3,975.8 ± 76.9 | 171.2 | 135 | 25,513 | identical |
| Open Logic | pdflatex | preamble | | | failed: could not precompile: ! Paragraph ended before \openout was complete. | 512 | | |
| Open Logic | pdflatex | draft | 31,189.5 ± 59.3 | 30,928.2 ± 42.3 | 261.3 |  |  | not-compared |
| Open Logic | pdflatex | externalize | 31,189.5 ± 59.3 | 27,253.4 ± 113.9 | 3,936.1 | 9,517 | 5,558,166 | identical |
| Open Logic | xelatex | preamble | | | failed: could not precompile: ! Paragraph ended before \openout was complete. | 516 | | |
| Open Logic | xelatex | draft | 32,549.9 ± 608.9 | 31,674.2 ± 1,751.8 | 875.6 |  |  | not-compared |
| Open Logic | xelatex | externalize | 32,549.9 ± 608.9 | 29,385.4 ± 3,729.1 | 3,164.4 | 8,988 | 5,749,790 | identical |
| Open Logic | lualatex | preamble | | | failed: could not precompile: ! Paragraph ended before \openout was complete. | 541 | | |
| Open Logic | lualatex | draft | 41,563.2 ± 110.4 | 41,018.1 ± 104.1 | 545.1 |  |  | not-compared |
| Open Logic | lualatex | externalize | 41,563.2 ± 110.4 | 37,494.0 ± 17.1 | 4,069.2 | 10,164 | 7,146,641 | identical |
| Stacks Project | pdflatex | preamble | 45,605.3 ± 929.9 | 45,516.3 ± 845.6 | 89.0 | 116 | 292 | identical |
| Stacks Project | pdflatex | draft | 45,605.3 ± 929.9 | 42,231.3 ± 937.8 | 3,374.1 |  |  | not-compared |
| Stacks Project | pdflatex | externalize | | | not-run: the document does not load tikz | 0 | | |

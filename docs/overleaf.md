# prumo and Overleaf

Overleaf builds your document on its own servers, so prumo cannot run that build. It can read
what the build leaves behind. Load the probe from the first line of your document, compile,
download the record file, and run `prumo time` on it on your machine. No TeX needed there.

## Steps

1. Download [`tex/prumo.sty`](../tex/prumo.sty) and upload it to the project, next to your
   main file.
2. Put this line above `\documentclass`:

   ```latex
   \RequirePackage{prumo}\PrumoEnvironments{tikzpicture,tblr}
   ```

   `\PrumoEnvironments` is optional. It names the environments you want timed.
3. Recompile. Under "Logs and output files", open "Other logs and files" and download
   `output.prumo`.
4. On your machine: `prumo time output.prumo`. Add `--html` for the page or `--json` for the
   data. `prumo graph output.prumo` gives the load tree.

Remove the line and the file when you are done. The probe does not change the PDF (that is
tested on every engine), but it is still a line in your document.

## What I checked, and what I did not

I have not tried this on overleaf.com. Here is what I did check:

- Overleaf's compile service is open source. It hands back every file the build wrote that
  was not part of the upload (`services/clsi/app/js/OutputFileFinder.js` in
  [overleaf/overleaf](https://github.com/overleaf/overleaf)). The editor lists all of them
  except `output.fls` and `output.fdb_latexmk`
  (`services/web/frontend/js/features/pdf-preview/util/file-list.ts`). So `output.prumo`
  should be in the list, and the `.fls` will not be.
- The build is latexmk with `-jobname=output` (`services/clsi/app/js/LatexRunner.js`). I ran
  that command here on a document with the probe on its first line, and `prumo time
  output.prumo` read the records. `tests/tex.rs::probe_loaded_from_the_first_line` does it on
  every test run.

If you try it on Overleaf, please tell me whether the file shows up.

## What you get

- The phases, the package and file times, the pages and the environments, as for a local
  build.
- No finalization. prumo did not start the process, so it cannot time it, and the header
  gives the time to the last record instead of the wall time.
- The records of latexmk's last pass.
- No row for the main file. The probe loads inside it, after it opened, so time the main
  file spends outside any package is in the phases but not in the unit table.
- No `fonts` or `packages`. Both need the `.fls`, which Overleaf does not offer.
- Overleaf's TeX Live must be 2021 or later (the probe needs a LaTeX kernel from 2021-06-01
  on). The project settings choose it.

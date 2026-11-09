#!/usr/bin/env bash
# Prints the Homebrew formula for a prumo release. The sha256 of each archive is read from
# the .sha256 files on the GitHub release, so the release must be complete first. The
# release workflow writes the result to kevinraymond/homebrew-tap as Formula/prumo.rb on
# every tag; by hand:
#   scripts/homebrew-formula.sh v0.1.0 > ../homebrew-tap/Formula/prumo.rb
set -euo pipefail
tag=${1:?usage: scripts/homebrew-formula.sh vX.Y.Z}
version=${tag#v}
repo=kevinraymond/prumo
base="https://github.com/$repo/releases/download/$tag"
sum() { curl -fsSL --retry 3 "$base/prumo-$tag-$1.tar.gz.sha256" | tr -d ' \r\n'; }
mac_arm=$(sum aarch64-apple-darwin)
mac_intel=$(sum x86_64-apple-darwin)
linux_arm=$(sum aarch64-unknown-linux-musl)
linux_intel=$(sum x86_64-unknown-linux-musl)
cat <<RUBY
# Written by scripts/homebrew-formula.sh in kevinraymond/prumo for $tag. Edit it there.
class Prumo < Formula
  desc "Build analyzer for LaTeX: time per package and page, fonts, needed packages"
  homepage "https://github.com/$repo"
  version "$version"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "$base/prumo-$tag-aarch64-apple-darwin.tar.gz"
      sha256 "$mac_arm"
    end
    on_intel do
      url "$base/prumo-$tag-x86_64-apple-darwin.tar.gz"
      sha256 "$mac_intel"
    end
  end

  on_linux do
    on_arm do
      url "$base/prumo-$tag-aarch64-unknown-linux-musl.tar.gz"
      sha256 "$linux_arm"
    end
    on_intel do
      url "$base/prumo-$tag-x86_64-unknown-linux-musl.tar.gz"
      sha256 "$linux_intel"
    end
  end

  def install
    bin.install "prumo"
  end

  def caveats
    <<~TEXT
      prumo measures the TeX installation on your PATH (TeX Live or MiKTeX) and reads the
      PDF with pdffonts from poppler (brew install poppler). prumo doctor says what it found.
    TEXT
  end

  test do
    assert_match "prumo #{version}", shell_output("#{bin}/prumo --version")
  end
end
RUBY

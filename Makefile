# Convenience targets; everything is also runnable by hand (see README.md).
.PHONY: test tex-tests verify-prune images social

test:
	cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test

# TeX-feature tests and demos inside the scheme-small image (both targets).
tex-tests: images
	scripts/tex_tests.sh prumo-tex
	scripts/tex_tests.sh prumo-tex-nocmsuper fonts_report_courier_and_type3

# The M4 gate: prune a full TeX Live to the fixtures, rebuild, compare. Slow.
verify-prune:
	docker image inspect prumo-dev:tl2026-full >/dev/null 2>&1 || \
	  printf 'FROM texlive/texlive:latest\nRUN apt-get update && apt-get install -y --no-install-recommends poppler-utils && rm -rf /var/lib/apt/lists/*\n' | docker build -q -t prumo-dev:tl2026-full -
	scripts/verify_prune.sh prumo-dev:tl2026-full

images:
	docker build -q --target clean -t prumo-tex -f docker/Dockerfile.tex .
	docker build -q --target nocmsuper -t prumo-tex-nocmsuper -f docker/Dockerfile.tex .

# The repository's social image, docs/img/social.png, in the full TeX Live image.
social:
	scripts/social.sh

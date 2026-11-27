# One command for the whole benchmark: `make -C bench`.
# Installs the pinned npm tools, fetches/generates and verifies the corpora,
# builds prunecss in release mode and runs every measurement.
.PHONY: all bench quick deps corpus clean

all: bench

deps: node_modules/.package-lock.json

node_modules/.package-lock.json: package.json package-lock.json
	npm ci

corpus: deps
	node scripts/fetch-corpus.mjs

bench: corpus
	node run.mjs $(ARGS)

# A fast smoke run (few iterations, no huge corpus). Not for quoting numbers.
quick: corpus
	node run.mjs --quick $(ARGS)

clean:
	rm -rf .work corpus/bootstrap corpus/bulma corpus/tailwind corpus/synthetic

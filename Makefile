.PHONY: all install

all: src/*
	cargo build

install: all conf/*
	mkdir -p /var/log/crypto-trader

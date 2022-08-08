all: init_test test

create:
	@cd ../fat32-fuse && make create

init:
	@cd ../fat32-fuse && make init

init_test:
	@cd ../fat32-fuse && make init_test

test:
	@export RUST_BACKTRACE=1 && cargo test -- --show-output

start:
	@cd ../fat32-fuse && make start

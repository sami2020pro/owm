BUILDFLAGS := --release

# To get the absolute path of project directory.
ROOT_DIR := $(shell dirname $(realpath $(firstword $(MAKEFILE_LIST))))

TARGET_DIR := /usr/bin
SHARE_DIR := /usr/share/xsessions

build: ## builds the project
	@cd $(ROOT_DIR) && cargo build ${BUILDFLAGS}

# See "man install" item -s for informations.
install: build ## builds the project and then installs the project
	@cp $(ROOT_DIR)/owm.desktop $(SHARE_DIR)/
	@install -s -Dm755 $(ROOT_DIR)/target/release/owm -t $(TARGET_DIR)/
	@echo "install: installed."

uninstall: ## uninstalls the project
	@rm $(SHARE_DIR)/owm.desktop
	@rm $(TARGET_DIR)/owm
	@echo "uninstall: uninstalled."

clean: ## removes the generated binaries
	@cd $(ROOT_DIR) && cargo clean
	@echo "clean: build files have been cleaned."

help: ## help
	@grep -E '^[a-zA-Z]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[37m%-30s\033[0m %s\n", $$1, $$2}'

# See below link, if you don't know about PHONY.
# - https://www.gnu.org/software/make/manual/html_node/Phony-Targets.html
.PHONY: build install uninstall clean help
.DEFAULT_GOAL := help

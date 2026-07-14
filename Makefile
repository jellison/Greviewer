# Greviewer make tasks.
# Bundling logic lives in bin/bundle (the source of truth); these targets wrap it.
# macOS only for bundle/install, matching bin/bundle's own guard.

APP_NAME := Greviewer
BUNDLE   := target/bundle/$(APP_NAME).app
# Override with `make install INSTALL_DIR=~/Applications` for a per-user install.
INSTALL_DIR := /Applications
INSTALLED   := $(INSTALL_DIR)/$(APP_NAME).app

.PHONY: check bundle install uninstall

# Run the full verification suite (fmt, clippy, tests).
check:
	bin/check

# Build the release binary and assemble target/bundle/Greviewer.app.
bundle:
	bin/bundle

# Bundle, then install into $(INSTALL_DIR) (default /Applications).
install: bundle
	@echo "==> Installing $(APP_NAME).app to $(INSTALL_DIR)"
	@mkdir -p "$(INSTALL_DIR)"
	@rm -rf "$(INSTALLED)"
	@cp -R "$(BUNDLE)" "$(INSTALLED)"
	@echo "==> Installed: $(INSTALLED)"

# Remove the installed app.
uninstall:
	@echo "==> Removing $(INSTALLED)"
	@rm -rf "$(INSTALLED)"

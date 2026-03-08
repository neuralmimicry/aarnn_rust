# Copyright 1996-2024 Cyberbotics Ltd.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     https://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Webots Makefile system
#
# You may add some variable definitions hereafter to customize the build process
# See documentation in $(WEBOTS_HOME_PATH)/resources/Makefile.include

# =================================================================================
# Project-local helpers to build the NAO Webots controller examples from this repo.
# This does NOT require a Webots project tree; it only expects Webots to be
# installed and reachable via WEBOTS_HOME (default: /usr/local/webots).
#
# Targets of interest:
#   make all                       # builds both controllers
#   make nao_nn_controller         # in-process FFI (links Rust cdylib)
#   make nao_nn_controller_uds     # out-of-process (UDS client only)
#
# What it does:
#   1) Build the Rust cdylib with feature `ffi_bridge` into target/<profile>
#   2) Compile the C++ controllers and link them against:
#        - Webots libCppController + libController
#        - For FFI variant: the exact libaarnn_rust.so just built
#      rpath is set so the executable can find both at runtime.
# =================================================================================

# Detect repo root from this file location (examples/..)
REPO_ROOT := $(abspath ..)

# Toolchain
CXX ?= g++
PROFILE ?= release
ENABLE_OPENMP ?= auto

TARGET_DIR := $(REPO_ROOT)/target/$(PROFILE)
RUST_FEATURES := ffi_bridge

# Webots installation (override WEBOTS_HOME in your environment if different)
WEBOTS_HOME ?= /usr/local/webots
WEBOTS_INC := \
  -I$(WEBOTS_HOME)/include \
  -I$(WEBOTS_HOME)/include/controller/c \
  -I$(WEBOTS_HOME)/include/controller/cpp
# Link against the C++ controller wrapper first, then the base controller lib
WEBOTS_LIB := -L$(WEBOTS_HOME)/lib/controller -lCppController -lController

# Rust FFI header and library
INC := -I$(REPO_ROOT)/include
# Link against the exact .so to avoid lib resolution ambiguity on some setups
LIB_SO := $(TARGET_DIR)/libaarnn_rust.so

# rpath setup:
#  - $$ORIGIN so the executable can find libaarnn_rust.so placed next to it
#  - Absolute path to Webots lib/controller for standalone runs outside Webots
RPATH_SELF := -Wl,-rpath,'$$ORIGIN'
RPATH_WEBOTS := -Wl,-rpath,$(WEBOTS_HOME)/lib/controller

OPENMP_CXXFLAGS :=
OPENMP_LDFLAGS :=
ifeq ($(ENABLE_OPENMP),1)
OPENMP_CXXFLAGS += -fopenmp
OPENMP_LDFLAGS += -fopenmp
else ifeq ($(ENABLE_OPENMP),true)
OPENMP_CXXFLAGS += -fopenmp
OPENMP_LDFLAGS += -fopenmp
else ifeq ($(ENABLE_OPENMP),auto)
OPENMP_SUPPORTED := $(shell printf 'int main(){return 0;}\n' | $(CXX) -std=c++17 -x c++ -fopenmp -o /tmp/nm_openmp_probe_makefile - >/dev/null 2>&1 && rm -f /tmp/nm_openmp_probe_makefile && echo 1 || (rm -f /tmp/nm_openmp_probe_makefile; echo 0))
ifeq ($(OPENMP_SUPPORTED),1)
OPENMP_CXXFLAGS += -fopenmp
OPENMP_LDFLAGS += -fopenmp
endif
endif

.PHONY: all nao_nn_controller nao_nn_controller_uds clean help install

all: nao_nn_controller nao_nn_controller_uds

# Target to install the UDS controller to a Webots project
# Usage: make install WEBOTS_PROJECT=/path/to/your/project
install: nao_nn_controller_uds
	@if [ -z "$(WEBOTS_PROJECT)" ]; then \
	  echo "Error: WEBOTS_PROJECT is not set."; \
	  echo "Usage: make install WEBOTS_PROJECT=/path/to/your/webots_project"; \
	  exit 1; \
	fi
	mkdir -p $(WEBOTS_PROJECT)/controllers/nao_nn_controller_uds
	cp $(TARGET_DIR)/nao_nn_controller_uds $(WEBOTS_PROJECT)/controllers/nao_nn_controller_uds/
	@echo "Successfully installed to $(WEBOTS_PROJECT)/controllers/nao_nn_controller_uds"

$(TARGET_DIR)/libaarnn_rust.so:
	@echo "[Rust] Building cdylib (profile=$(PROFILE))..."
ifneq ($(PROFILE),release)
	cargo build --features $(RUST_FEATURES)
else
	cargo build --release --features $(RUST_FEATURES)
endif
	@# Verify FFI symbols are exported (best-effort)
	@if command -v nm >/dev/null 2>&1; then \
	  if ! nm -D $(TARGET_DIR)/libaarnn_rust.so | grep -q " nm_init$$"; then \
	    echo "[Error] nm_init not found in $(TARGET_DIR)/libaarnn_rust.so. Did the build include --features ffi_bridge?"; \
	    exit 2; \
	  fi; \
	fi

nao_nn_controller: $(TARGET_DIR)/libaarnn_rust.so
	@mkdir -p $(TARGET_DIR)
	@# Ensure the cdylib actually exports the FFI symbols; if not, (re)build with features
	@if ! nm -D $(LIB_SO) 2>/dev/null | grep -q " nm_init$$"; then \
	  echo "[Rust] libaarnn_rust.so missing nm_init; rebuilding with --features $(RUST_FEATURES)..."; \
	  if [ "$(PROFILE)" = "release" ]; then cargo build --release --features $(RUST_FEATURES); else cargo build --features $(RUST_FEATURES); fi; \
	  if ! nm -D $(LIB_SO) 2>/dev/null | grep -q " nm_init$$"; then \
	    echo "[Error] nm_init still missing from $(LIB_SO). Please ensure feature 'ffi_bridge' is enabled."; \
	    exit 2; \
	  fi; \
	fi
	@echo "[C++] Building examples/nao_nn_controller.cpp -> $(TARGET_DIR)/nao_nn_controller"
	$(CXX) -std=c++17 -O2 $(OPENMP_CXXFLAGS) $(INC) $(WEBOTS_INC) \
	  $(REPO_ROOT)/examples/nao_nn_controller.cpp \
	  $(LIB_SO) -Wl,--no-as-needed $(WEBOTS_LIB) $(OPENMP_LDFLAGS) $(RPATH_SELF) $(RPATH_WEBOTS) \
	  -o $(TARGET_DIR)/nao_nn_controller
	@echo "Done. Run with: LD_LIBRARY_PATH=$(TARGET_DIR):$(WEBOTS_HOME)/lib/controller $(TARGET_DIR)/nao_nn_controller"

nao_nn_controller_uds:
	@mkdir -p $(TARGET_DIR)
	@echo "[C++] Building examples/nao_nn_controller_uds.cpp -> $(TARGET_DIR)/nao_nn_controller_uds"
	$(CXX) -std=c++17 -O2 $(OPENMP_CXXFLAGS) $(INC) $(WEBOTS_INC) \
	  $(REPO_ROOT)/examples/nao_nn_controller_uds.cpp \
	  $(WEBOTS_LIB) $(OPENMP_LDFLAGS) $(RPATH_WEBOTS) \
	  -o $(TARGET_DIR)/nao_nn_controller_uds
	@echo "Done. Install into your Webots project controllers folder as 'nao_nn_controller_uds' and set NM_SOCK if needed."

clean:
	rm -f $(TARGET_DIR)/nao_nn_controller
	rm -f $(TARGET_DIR)/nao_nn_controller_uds

help:
	@echo "Targets:"
	@echo "  nao_nn_controller    Build the Webots NAO NN controller (default)."
	@echo "Variables:"
	@echo "  PROFILE=release|debug (default: release)"
	@echo "  WEBOTS_HOME=/path/to/webots (default: /usr/local/webots)"
	@echo "  ENABLE_OPENMP=auto|1|0 (default: auto)"

# ---------------------------------------------------------------------------------
# Webots global Makefile.include (kept for compatibility if WEBOTS_HOME is set).
# Use -include so a missing file doesn't break local builds.
null :=
space := $(null) $(null)
WEBOTS_HOME_PATH?=$(subst $(space),\ ,$(strip $(subst \,/,$(WEBOTS_HOME))))
-include $(WEBOTS_HOME_PATH)/resources/Makefile.include

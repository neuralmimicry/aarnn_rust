# Standard Webots Makefile for C++ controller
CXX_SOURCES = nao_nn_controller_uds.cpp

# Repo root is 3 levels up from this controller directory
REPO_ROOT := ../../..

# Add repo include directory and Webots C/C++ API headers
INCLUDE = -I$(REPO_ROOT)/include -I$(WEBOTS_HOME)/include/controller/c -I$(WEBOTS_HOME)/include/controller/cpp

# Standard Webots Makefile include
# This handles WEBOTS_HOME and all standard paths/rules
WEBOTS_HOME ?= /usr/local/webots
include $(WEBOTS_HOME)/resources/Makefile.include

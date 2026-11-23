# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
from __future__ import annotations

from waitress import serve

from .app import Settings, create_app


if __name__ == "__main__":
    settings = Settings.from_env()
    serve(create_app(settings), host=settings.host, port=settings.port)

FROM python:3.12-slim-bookworm

WORKDIR /app

# Install uv
COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

# Install chatixia package
COPY agent/ /app/
RUN uv sync --frozen --no-dev

# Default: validate manifest exists then run
ENTRYPOINT ["uv", "run", "chatixia"]
CMD ["run"]

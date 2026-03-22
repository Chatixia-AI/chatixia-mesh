FROM python:3.12-slim-bookworm

# System deps for psycopg binary
RUN apt-get update && apt-get install -y --no-install-recommends libpq5 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install uv
COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

# Install chatixia package
COPY agent/ /app/
RUN uv sync --frozen --no-dev

# Default: validate manifest exists then run
ENTRYPOINT ["uv", "run", "chatixia"]
CMD ["run"]

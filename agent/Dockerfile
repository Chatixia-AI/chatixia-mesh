FROM python:3.13-slim-bookworm

# System deps for psycopg binary
RUN apt-get update && apt-get install -y --no-install-recommends libpq5 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install chatixia package
COPY agent/ /app/
RUN pip install --no-cache-dir .

# Default: validate manifest exists then run
ENTRYPOINT ["chatixia"]
CMD ["run"]

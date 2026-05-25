ARG BUILD_FROM=ghcr.io/home-assistant/aarch64-base-python:3.12-alpine3.20
FROM $BUILD_FROM

ENV LANG=C.UTF-8 \
    PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1

# Pillow runtime deps (jpeg/zlib/freetype). build-base only needed if wheels miss.
RUN apk add --no-cache \
        jpeg-dev zlib-dev freetype-dev \
        ttf-dejavu \
    && true

WORKDIR /opt/app

COPY requirements.txt /opt/app/requirements.txt
RUN pip install --no-cache-dir -r /opt/app/requirements.txt

COPY app /opt/app/app
COPY run.sh /run.sh
RUN chmod +x /run.sh

EXPOSE 8099
CMD ["/run.sh"]

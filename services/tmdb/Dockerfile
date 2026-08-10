# A static binary on `scratch`, like the relay's — but with one difference that
# matters: this service *does* keep state. The image cache is a directory of
# JPEGs, and it is the whole reason the service is affordable, so it belongs on
# a volume rather than inside the layer.
FROM golang:1.24-alpine AS build
WORKDIR /src
# Copied first so a dependency layer survives a code change. There are no
# dependencies at all here, which makes the layer trivially cacheable and is
# itself the point — see go.mod.
COPY go.mod ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -trimpath -ldflags "-s -w" -o /tmdb .

FROM scratch
COPY --from=build /tmdb /tmdb
# TLS to api.themoviedb.org needs the trust store, which `scratch` has none of.
# Taken from the build stage rather than installed, so the final image still
# carries no package manager and nothing to keep patched.
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
# The cache directory has to exist and be writable by the unprivileged uid.
# `scratch` has no shell to `mkdir` with, so it arrives as a volume — which is
# where it belongs anyway, since losing it on every deploy would mean re-fetching
# every poster.
VOLUME ["/cache"]
USER 65534:65534
ENV TMDB_CACHE_DIR=/cache/images
EXPOSE 8090
ENTRYPOINT ["/tmdb"]

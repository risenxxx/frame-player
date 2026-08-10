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
# The cache directory is made **here**, where there is still a shell. `scratch`
# has none, so a `VOLUME` declared against a path that does not exist in the
# image is created by Docker owned by root — and the unprivileged user below
# then cannot create anything inside it. That is the whole of
# `mkdir /cache/images: permission denied` at first start.
RUN mkdir -p /cache/images

FROM scratch
COPY --from=build /tmdb /tmdb
# TLS to api.themoviedb.org needs the trust store, which `scratch` has none of.
# Taken from the build stage rather than installed, so the final image still
# carries no package manager and nothing to keep patched.
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
# Carried over from the build stage **with its ownership**, which is what makes
# the unprivileged user able to write to it. A fresh *named* volume is seeded
# from the image at this path — contents, ownership and mode — so `-v
# name:/cache` inherits this and works with no setup.
#
# A **bind mount does not**: there the host directory's ownership wins and the
# image's is irrelevant. Dokploy and most Compose setups bind-mount by default,
# so if the error comes back after this, the host path is the thing to fix —
# `chown -R 65534:65534 <path>` — or drop the mount and let a named volume do
# it. See the README.
COPY --from=build --chown=65534:65534 /cache /cache
VOLUME ["/cache"]
USER 65534:65534
ENV TMDB_CACHE_DIR=/cache/images
EXPOSE 8090
ENTRYPOINT ["/tmdb"]

(() => {
  const SERVICE_ACCESS_NONE = "none";
  const SERVICE_ACCESS_REQUEST = "request";
  const SERVICE_ACCESS_OBSERVE = "observe";
  const SERVICE_ACCESS_USE = "use";
  const SERVICE_ACCESS_CONTROL = "control";

  const SERVICE_ACCESS_ORDER = {
    [SERVICE_ACCESS_NONE]: 0,
    [SERVICE_ACCESS_REQUEST]: 1,
    [SERVICE_ACCESS_OBSERVE]: 2,
    [SERVICE_ACCESS_USE]: 3,
    [SERVICE_ACCESS_CONTROL]: 4
  };

  const DEFAULT_SERVICE_CATALOG = {
    aarnn: {
      public_access_level: SERVICE_ACCESS_REQUEST
    },
    billing: {
      public_access_level: SERVICE_ACCESS_NONE
    }
  };

  const DEFAULT_AUTHENTICATED_GRANTS = {
    aarnn: SERVICE_ACCESS_REQUEST,
    billing: SERVICE_ACCESS_USE
  };

  const normalizeAccessLevel = (value, fallback = SERVICE_ACCESS_NONE) => {
    const cleaned = String(value || fallback).trim().toLowerCase() || fallback;
    return Object.prototype.hasOwnProperty.call(SERVICE_ACCESS_ORDER, cleaned) ? cleaned : fallback;
  };

  const accessAtLeast = (current, required) => (
    SERVICE_ACCESS_ORDER[normalizeAccessLevel(current)] >= SERVICE_ACCESS_ORDER[normalizeAccessLevel(required)]
  );

  const maxAccessLevel = (...levels) => levels.reduce((best, candidate) => (
    SERVICE_ACCESS_ORDER[normalizeAccessLevel(candidate)] > SERVICE_ACCESS_ORDER[best]
      ? normalizeAccessLevel(candidate)
      : best
  ), SERVICE_ACCESS_NONE);

  const coerceGroups = value => {
    const rawValues = Array.isArray(value) ? value : typeof value === "string" ? value.split(",") : [];
    const seen = new Set();
    return rawValues.reduce((groups, item) => {
      const cleaned = String(item || "").trim().toLowerCase();
      if (!cleaned || seen.has(cleaned)) {
        return groups;
      }
      seen.add(cleaned);
      groups.push(cleaned);
      return groups;
    }, []);
  };

  const buildFallbackAccess = (sessionData = {}) => {
    const authenticated = Boolean(sessionData && (sessionData.authenticated ?? sessionData.user));
    const role = String((sessionData && sessionData.role) || "user").trim().toLowerCase() || "user";
    const groups = coerceGroups(sessionData && sessionData.groups);
    const isAdmin = Boolean(sessionData && sessionData.is_admin) || role === "admin" || groups.includes("admin");
    return Object.keys(DEFAULT_SERVICE_CATALOG).reduce((services, serviceKey) => {
      const publicAccessLevel = normalizeAccessLevel(DEFAULT_SERVICE_CATALOG[serviceKey].public_access_level);
      const grantedAccessLevel = isAdmin
        ? SERVICE_ACCESS_CONTROL
        : authenticated
          ? normalizeAccessLevel(DEFAULT_AUTHENTICATED_GRANTS[serviceKey], SERVICE_ACCESS_NONE)
          : SERVICE_ACCESS_NONE;
      const visibleAccessLevel = maxAccessLevel(grantedAccessLevel, publicAccessLevel);
      services[serviceKey] = {
        service_key: serviceKey,
        access_level: grantedAccessLevel,
        public_access_level: publicAccessLevel,
        visible_access_level: visibleAccessLevel,
        visible: visibleAccessLevel !== SERVICE_ACCESS_NONE,
        can_request: accessAtLeast(visibleAccessLevel, SERVICE_ACCESS_REQUEST),
        can_observe: accessAtLeast(visibleAccessLevel, SERVICE_ACCESS_OBSERVE),
        can_use: accessAtLeast(grantedAccessLevel, SERVICE_ACCESS_USE),
        can_control: accessAtLeast(grantedAccessLevel, SERVICE_ACCESS_CONTROL)
      };
      return services;
    }, {});
  };

  const coerceExplicitServiceAccess = (sessionData = {}) => {
    const raw = sessionData && sessionData.service_access;
    if (!raw) {
      return {};
    }
    const items = Array.isArray(raw)
      ? raw
      : Object.entries(raw).map(([serviceKey, value]) => (
        value && typeof value === "object"
          ? { service_key: serviceKey, ...value }
          : { service_key: serviceKey, access_level: value }
      ));

    return items.reduce((services, entry) => {
      if (!entry || typeof entry !== "object") {
        return services;
      }
      const serviceKey = String(entry.service_key || entry.key || "").trim().toLowerCase();
      if (!serviceKey) {
        return services;
      }
      const publicAccessLevel = normalizeAccessLevel(
        entry.public_access_level ?? (DEFAULT_SERVICE_CATALOG[serviceKey] && DEFAULT_SERVICE_CATALOG[serviceKey].public_access_level),
        SERVICE_ACCESS_NONE
      );
      const accessLevel = normalizeAccessLevel(entry.access_level ?? entry.level, SERVICE_ACCESS_NONE);
      const visibleAccessLevel = normalizeAccessLevel(
        entry.visible_access_level,
        maxAccessLevel(accessLevel, publicAccessLevel)
      );
      services[serviceKey] = {
        ...entry,
        service_key: serviceKey,
        access_level: accessLevel,
        public_access_level: publicAccessLevel,
        visible_access_level: visibleAccessLevel,
        visible: typeof entry.visible === "boolean" ? entry.visible : visibleAccessLevel !== SERVICE_ACCESS_NONE,
        can_request: typeof entry.can_request === "boolean" ? entry.can_request : accessAtLeast(visibleAccessLevel, SERVICE_ACCESS_REQUEST),
        can_observe: typeof entry.can_observe === "boolean" ? entry.can_observe : accessAtLeast(visibleAccessLevel, SERVICE_ACCESS_OBSERVE),
        can_use: typeof entry.can_use === "boolean" ? entry.can_use : accessAtLeast(accessLevel, SERVICE_ACCESS_USE),
        can_control: typeof entry.can_control === "boolean" ? entry.can_control : accessAtLeast(accessLevel, SERVICE_ACCESS_CONTROL)
      };
      return services;
    }, {});
  };

  const getServiceAccessMap = (sessionData = {}) => {
    const fallback = buildFallbackAccess(sessionData);
    const explicit = coerceExplicitServiceAccess(sessionData);
    const serviceKeys = new Set([...Object.keys(fallback), ...Object.keys(explicit)]);
    return Array.from(serviceKeys).reduce((services, serviceKey) => {
      services[serviceKey] = {
        ...(fallback[serviceKey] || {
          service_key: serviceKey,
          access_level: SERVICE_ACCESS_NONE,
          public_access_level: SERVICE_ACCESS_NONE,
          visible_access_level: SERVICE_ACCESS_NONE,
          visible: false,
          can_request: false,
          can_observe: false,
          can_use: false,
          can_control: false
        }),
        ...(explicit[serviceKey] || {}),
        service_key: serviceKey
      };
      return services;
    }, {});
  };

  const getServiceAccess = (sessionData = {}, serviceKey) => (
    getServiceAccessMap(sessionData)[String(serviceKey || "").trim().toLowerCase()]
    || {
      service_key: String(serviceKey || "").trim().toLowerCase(),
      access_level: SERVICE_ACCESS_NONE,
      public_access_level: SERVICE_ACCESS_NONE,
      visible_access_level: SERVICE_ACCESS_NONE,
      visible: false,
      can_request: false,
      can_observe: false,
      can_use: false,
      can_control: false
    }
  );

  window.NMServiceAccess = {
    SERVICE_ACCESS_NONE,
    SERVICE_ACCESS_REQUEST,
    SERVICE_ACCESS_OBSERVE,
    SERVICE_ACCESS_USE,
    SERVICE_ACCESS_CONTROL,
    normalizeAccessLevel,
    accessAtLeast,
    getServiceAccessMap,
    getServiceAccess,
    getVisibleServices: sessionData => Object.values(getServiceAccessMap(sessionData || {})).filter(service => service.visible).map(service => service.service_key).sort(),
    isServiceVisible: (sessionData = {}, serviceKey) => Boolean(getServiceAccess(sessionData, serviceKey).visible),
    canObserveService: (sessionData = {}, serviceKey) => Boolean(getServiceAccess(sessionData, serviceKey).can_observe),
    canUseService: (sessionData = {}, serviceKey) => Boolean(getServiceAccess(sessionData, serviceKey).can_use),
    canControlService: (sessionData = {}, serviceKey) => Boolean(getServiceAccess(sessionData, serviceKey).can_control)
  };
})();

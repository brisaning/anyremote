//! `anyremote-session` — Máquina de estados de sesión y permisos (D-07, patrón H-RD-03).
//! Trazabilidad: ver specs/04-trazabilidad.md. Clean-room D-01: sin copia de código AGPL.

/// Estados del ciclo de vida de una sesión (spec §9, D-07).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    Handshaking,
    AwaitingPermission,
    Active,
    Closing,
    Closed,
}

/// Eventos que impulsan las transiciones. Denegación por defecto: sin `PermissionGranted` no hay `Active`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionEvent {
    HandshakeOk,
    PermissionGranted,
    PermissionDenied,
    StartClose,
    Closed,
}

#[derive(Debug, PartialEq)]
pub enum SessionError {
    IllegalTransition(SessionState, SessionEvent),
}

impl SessionState {
    pub fn transition(self, event: SessionEvent) -> Result<SessionState, SessionError> {
        use SessionEvent as E;
        use SessionState as S;
        let next = match (self, event) {
            (S::Handshaking, E::HandshakeOk) => S::AwaitingPermission,
            (S::AwaitingPermission, E::PermissionGranted) => S::Active,
            (S::AwaitingPermission, E::PermissionDenied) => S::Closing,
            (S::Active, E::StartClose) | (S::AwaitingPermission, E::StartClose) | (S::Handshaking, E::StartClose) => S::Closing,
            (S::Closing, E::Closed) => S::Closed,
            (s, e) => return Err(SessionError::IllegalTransition(s, e)),
        };
        Ok(next)
    }
}

/// Permisos granulares por sesión; todos denegados por defecto (D-07).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct PermissionSet {
    pub keyboard: bool,
    pub mouse: bool,
    pub clipboard: bool,
    pub audio: bool,
    pub files: bool,
}

impl PermissionSet {
    pub const fn none() -> Self {
        Self {
            keyboard: false,
            mouse: false,
            clipboard: false,
            audio: false,
            files: false,
        }
    }
}

/// Métricas básicas de enlace de una sesión (gate C1; base de RF-05/D-10).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LinkStats {
    pub heartbeats_ok: u64,
    pub last_rtt_ms: Option<u64>,
    pub min_rtt_ms: Option<u64>,
}

impl LinkStats {
    pub fn record_rtt(&mut self, rtt_ms: u64) {
        self.heartbeats_ok += 1;
        self.last_rtt_ms = Some(rtt_ms);
        self.min_rtt_ms = Some(self.min_rtt_ms.map_or(rtt_ms, |m| m.min(rtt_ms)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SessionEvent as E;
    use SessionState as S;

    #[test]
    fn happy_path_reaches_active() {
        let s = S::Handshaking.transition(E::HandshakeOk).unwrap();
        assert_eq!(s, S::AwaitingPermission);
        assert_eq!(s.transition(E::PermissionGranted).unwrap(), S::Active);
        assert_eq!(S::Active.transition(E::StartClose).unwrap(), S::Closing);
        assert_eq!(S::Closing.transition(E::Closed).unwrap(), S::Closed);
    }

    #[test]
    fn denial_closes_without_activating() {
        let s = S::Handshaking.transition(E::HandshakeOk).unwrap();
        assert_eq!(s.transition(E::PermissionDenied).unwrap(), S::Closing);
        assert_eq!(S::Closing.transition(E::Closed).unwrap(), S::Closed);
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        assert_eq!(
            S::Handshaking.transition(E::PermissionGranted),
            Err(SessionError::IllegalTransition(S::Handshaking, E::PermissionGranted))
        );
        assert_eq!(
            S::Closed.transition(E::HandshakeOk),
            Err(SessionError::IllegalTransition(S::Closed, E::HandshakeOk))
        );
    }

    #[test]
    fn permissions_default_deny() {
        let p = PermissionSet::default();
        assert!(!p.keyboard && !p.mouse && !p.clipboard && !p.audio && !p.files);
    }

    #[test]
    fn link_stats_track_rtt() {
        let mut s = LinkStats::default();
        s.record_rtt(12);
        s.record_rtt(5);
        s.record_rtt(9);
        assert_eq!(s.heartbeats_ok, 3);
        assert_eq!(s.last_rtt_ms, Some(9));
        assert_eq!(s.min_rtt_ms, Some(5));
    }
}

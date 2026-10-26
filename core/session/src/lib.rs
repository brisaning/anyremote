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
        use SessionEvent::*;
        use SessionState::*;
        let next = match (self, event) {
            (Handshaking, HandshakeOk) => AwaitingPermission,
            (AwaitingPermission, PermissionGranted) => Active,
            (AwaitingPermission, PermissionDenied) => Closing,
            (Active, StartClose) | (AwaitingPermission, StartClose) | (Handshaking, StartClose) => Closing,
            (Closing, Closed) => Closed,
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

#[cfg(test)]
mod tests {
    use super::*;
    use SessionEvent::*;
    use SessionState::*;

    #[test]
    fn happy_path_reaches_active() {
        let s = Handshaking.transition(HandshakeOk).unwrap();
        assert_eq!(s, AwaitingPermission);
        assert_eq!(s.transition(PermissionGranted).unwrap(), Active);
        assert_eq!(Active.transition(StartClose).unwrap(), Closing);
        assert_eq!(Closing.transition(Closed).unwrap(), Closed);
    }

    #[test]
    fn denial_closes_without_activating() {
        let s = Handshaking.transition(HandshakeOk).unwrap();
        assert_eq!(s.transition(PermissionDenied).unwrap(), Closing);
        assert_eq!(Closing.transition(Closed).unwrap(), Closed);
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        assert_eq!(
            Handshaking.transition(PermissionGranted),
            Err(SessionError::IllegalTransition(Handshaking, PermissionGranted))
        );
        assert_eq!(
            Closed.transition(HandshakeOk),
            Err(SessionError::IllegalTransition(Closed, HandshakeOk))
        );
    }

    #[test]
    fn permissions_default_deny() {
        let p = PermissionSet::default();
        assert!(!p.keyboard && !p.mouse && !p.clipboard && !p.audio && !p.files);
    }
}

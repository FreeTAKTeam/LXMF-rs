mod issue_612_permission_service_compat_tests {
    use super::*;

    #[test]
    fn permission_target_aliases_match_python_case_sensitivity() {
        let node = ReticulumGitNode::default();

        assert_eq!(
            node.parse_permission("READ:all"),
            Some((ReticulumGitNode::PERM_READ, PermissionTarget::All))
        );
        assert_eq!(node.parse_permission("r:ALL"), None);
        assert_eq!(node.parse_permission("r:Everyone"), None);
        assert!(node.permissions_from_allowed_input(Some("r:ALL")).read.is_empty());
    }
}

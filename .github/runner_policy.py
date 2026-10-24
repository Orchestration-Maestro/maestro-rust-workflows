"""Shared secret-reference policy for CI selection and ephemeral runner admission."""
import re


def require_secret_free(source):
    """Refuse inherited, declared or referenced repository secrets in workflow text."""
    if re.search(r'^\s*secrets\s*:', source, re.MULTILINE):
        raise ValueError('workflow declares or inherits secrets')
    for expression in re.findall(r'\$\{\{(.*?)\}\}', source, re.DOTALL):
        allowed = re.sub(r'\bsecrets\.GITHUB_TOKEN\b', '', expression)
        if re.search(r'\bsecrets\b', allowed, re.IGNORECASE):
            raise ValueError('workflow references repository secrets')

"""Tests for context provider execution."""

from pathlib import Path
from unittest.mock import MagicMock, patch

from tambour.config import Config, ContextProviderConfig
from tambour.context import ContextCollector, ContextRequest


class TestContextCollector:
    """Tests for ContextCollector."""

    def test_execute_provider_generates_env_vars(self):
        """Test that provider options are passed as env vars."""
        # Setup config
        provider = ContextProviderConfig(
            name="tree",
            run="echo test",
            options={
                "exclude": [".git", "node_modules"],
                "depth": 3,
                "custom_val": "hello",
            },
        )
        config = MagicMock(spec=Config)
        config.get_enabled_context_providers.return_value = [provider]
        
        collector = ContextCollector(config)
        request = ContextRequest(prompt="test prompt")

        # Mock subprocess.run to capture env
        with patch("subprocess.run") as mock_run:
            mock_run.return_value.returncode = 0
            mock_run.return_value.stdout = "output"
            
            collector._execute_provider(provider, request)
            
            # Check env vars in call args
            call_args = mock_run.call_args
            env = call_args.kwargs["env"]
            
            # Verify options are converted to env vars
            assert env["TREE_EXCLUDE"] == ".git,node_modules"
            assert env["TREE_DEPTH"] == "3"
            assert env["TREE_CUSTOM_VAL"] == "hello"
            
            # Verify standard env vars
            assert env["TAMBOUR_PROMPT"] == "test prompt"

    def test_execute_provider_with_defaults(self):
        """Test execution with no options."""
        provider = ContextProviderConfig(
            name="simple",
            run="echo simple",
        )
        
        collector = ContextCollector(MagicMock(spec=Config))
        request = ContextRequest(prompt="test")
        
        with patch("subprocess.run") as mock_run:
            mock_run.return_value.returncode = 0
            
            collector._execute_provider(provider, request)
            
            env = mock_run.call_args.kwargs["env"]
            
            # Should not have random env vars
            # But standard ones should be there
            assert env["TAMBOUR_PROMPT"] == "test"
            
            # Ensure no crash on empty options

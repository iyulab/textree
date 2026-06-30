// src-host/tests/Textree.Host.Tests/Telemetry/TelemetryWiringTests.cs
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.Extensions.DependencyInjection;
using Textree.Host.Telemetry;
using Xunit;

public class TelemetryWiringTests : IClassFixture<WebApplicationFactory<Program>>
{
    private readonly WebApplicationFactory<Program> _factory;
    public TelemetryWiringTests(WebApplicationFactory<Program> factory) => _factory = factory;

    [Fact]
    public void Emitter_is_registered_and_defaults_to_noop_without_connection_string()
    {
        // No TEXTREE_TELEMETRY_CONNECTION set in the test environment → must resolve as the no-op.
        using var scope = _factory.Services.CreateScope();
        var emitter = scope.ServiceProvider.GetRequiredService<ITelemetryEmitter>();
        Assert.IsType<NullTelemetryEmitter>(emitter);
    }
}

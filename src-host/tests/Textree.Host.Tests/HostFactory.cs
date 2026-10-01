using System.Net.Http.Headers;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Textree.Host;

/// <summary>
/// The host as the desktop app starts it: with a per-run token, which the clients it makes carry.
/// Test factories derive from this and call <c>base.ConfigureWebHost</c>.
/// </summary>
public class HostFactory : WebApplicationFactory<Program>
{
    public const string Token = "test-token-0123456789abcdef0123456789abcdef";

    protected override void ConfigureWebHost(IWebHostBuilder builder) =>
        builder.UseSetting(HostToken.Setting, Token);

    protected override void ConfigureClient(HttpClient client)
    {
        base.ConfigureClient(client);
        client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", Token);
    }
}

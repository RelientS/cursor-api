// Token 管理功能
function saveAuthToken(token) {
  const expiryTime = new Date().getTime() + (24 * 60 * 60 * 1000); // 24小时后过期
  localStorage.setItem('authToken', token);
  localStorage.setItem('authTokenExpiry', expiryTime);
}

function getAuthToken() {
  const token = localStorage.getItem('authToken');
  const expiry = localStorage.getItem('authTokenExpiry');

  if (!token || !expiry) {
    return null;
  }

  if (new Date().getTime() > parseInt(expiry)) {
    localStorage.removeItem('authToken');
    localStorage.removeItem('authTokenExpiry');
    return null;
  }

  return token;
}

// 消息显示功能
function showMessage(elementId, text, isError = false) {
  const msg = document.getElementById(elementId);
  msg.className = `message ${isError ? 'error' : 'success'}`;
  msg.textContent = text;
}

function showGlobalMessage(text, isError = false) {
  showMessage('message', text, isError);
}

// Token 输入框自动填充和事件绑定
function initializeTokenHandling(inputId) {
  document.addEventListener('DOMContentLoaded', () => {
    const authToken = getAuthToken();
    if (authToken) {
      document.getElementById(inputId).value = authToken;
    }
  });

  document.getElementById(inputId).addEventListener('change', (e) => {
    if (e.target.value) {
      saveAuthToken(e.target.value);
    } else {
      localStorage.removeItem('authToken');
      localStorage.removeItem('authTokenExpiry');
    }
  });
}

// API 请求通用处理
async function makeAuthenticatedRequest(url, options = {}) {
  const tokenId = options.tokenId || 'authToken';
  const token = document.getElementById(tokenId).value;

  if (!token) {
    showGlobalMessage('请输入 AUTH_TOKEN', true);
    return null;
  }

  const defaultOptions = {
    method: 'POST',
    headers: {
      'Authorization': `Bearer ${token}`,
      'Content-Type': 'application/json'
    }
  };

  try {
    const response = await fetch(url, { ...defaultOptions, ...options });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    return await response.json();
  } catch (error) {
    showGlobalMessage(`请求失败: ${error.message}`, true);
    return null;
  }
}